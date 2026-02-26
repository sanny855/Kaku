use crate::customglyph::{BlockAlpha, BlockCoord, Poly, PolyCommand, PolyStyle};
use crate::termwindow::box_model::*;
use crate::termwindow::render::corners::{
    BOTTOM_LEFT_ROUNDED_CORNER, BOTTOM_RIGHT_ROUNDED_CORNER, TOP_LEFT_ROUNDED_CORNER,
    TOP_RIGHT_ROUNDED_CORNER,
};
use crate::termwindow::{DimensionContext, RenderFrame, TermWindowNotif};
use crate::utilsprites::RenderMetrics;
use ::window::bitmaps::atlas::OutOfTextureSpace;
use ::window::WindowOps;
use anyhow::Context;
use config::Dimension;
use smol::Timer;
use std::collections::HashSet;
use std::time::{Duration, Instant};
use wezterm_font::ClearShapeCache;
use window::color::LinearRgba;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllowImage {
    Yes,
    Scale(usize),
    No,
}

impl crate::TermWindow {
    pub fn paint_impl(&mut self, frame: &mut RenderFrame) -> anyhow::Result<()> {
        self.num_frames += 1;
        // If nothing on screen needs animating, then we can avoid
        // invalidating as frequently
        *self.has_animation.borrow_mut() = None;
        // Start with the assumption that we should allow images to render
        self.allow_images = AllowImage::Yes;

        let start = Instant::now();

        {
            let diff = start.duration_since(self.last_fps_check_time);
            if diff > Duration::from_secs(1) {
                let seconds = diff.as_secs_f32();
                self.fps = self.num_frames as f32 / seconds;
                self.num_frames = 0;
                self.last_fps_check_time = start;
            }
        }

        'pass: for pass in 0.. {
            match self.paint_pass() {
                Ok(_) => match self.render_state.as_mut().unwrap().allocated_more_quads() {
                    Ok(allocated) => {
                        if !allocated {
                            break 'pass;
                        }
                        self.invalidate_fancy_tab_bar();
                        self.invalidate_modal();
                    }
                    Err(err) => {
                        log::error!("{:#}", err);
                        break 'pass;
                    }
                },
                Err(err) => {
                    if let Some(&OutOfTextureSpace {
                        size: Some(size),
                        current_size,
                    }) = err.root_cause().downcast_ref::<OutOfTextureSpace>()
                    {
                        let result = if pass == 0 {
                            // Let's try clearing out the atlas and trying again
                            // self.clear_texture_atlas()
                            log::trace!("recreate_texture_atlas");
                            self.recreate_texture_atlas(Some(current_size))
                        } else {
                            log::trace!("grow texture atlas to {}", size);
                            self.recreate_texture_atlas(Some(size))
                        };
                        self.invalidate_fancy_tab_bar();
                        self.invalidate_modal();

                        if let Err(err) = result {
                            self.allow_images = match self.allow_images {
                                AllowImage::Yes => AllowImage::Scale(2),
                                AllowImage::Scale(2) => AllowImage::Scale(4),
                                AllowImage::Scale(4) => AllowImage::Scale(8),
                                AllowImage::Scale(8) => AllowImage::No,
                                AllowImage::No | _ => {
                                    log::error!(
                                        "Failed to {} texture: {}",
                                        if pass == 0 { "clear" } else { "resize" },
                                        err
                                    );
                                    break 'pass;
                                }
                            };

                            log::info!(
                                "Not enough texture space ({:#}); \
                                     will retry render with {:?}",
                                err,
                                self.allow_images,
                            );
                        }
                    } else if err.root_cause().downcast_ref::<ClearShapeCache>().is_some() {
                        self.invalidate_fancy_tab_bar();
                        self.invalidate_modal();
                        self.shape_generation += 1;
                        self.shape_cache.borrow_mut().clear();
                        self.line_to_ele_shape_cache.borrow_mut().clear();
                    } else {
                        log::error!("paint_pass failed: {:#}", err);
                        break 'pass;
                    }
                }
            }
        }

        log::debug!("paint_impl before call_draw elapsed={:?}", start.elapsed());

        self.call_draw(frame)?;
        self.last_frame_duration = start.elapsed();
        log::debug!(
            "paint_impl elapsed={:?}, fps={}",
            self.last_frame_duration,
            self.fps
        );
        metrics::histogram!("gui.paint.impl").record(self.last_frame_duration);
        metrics::histogram!("gui.paint.impl.rate").record(1.);

        // If self.has_animation is some, then the last render detected
        // image attachments with multiple frames, so we also need to
        // invalidate the viewport when the next frame is due
        if self.focused.is_some() {
            if let Some(next_due) = *self.has_animation.borrow() {
                let prior = self.scheduled_animation.borrow_mut().take();
                match prior {
                    Some(prior) if prior <= next_due => {
                        // Already due before that time
                    }
                    _ => {
                        self.scheduled_animation.borrow_mut().replace(next_due);
                        let window = self.window.clone().take().unwrap();
                        promise::spawn::spawn(async move {
                            Timer::at(next_due).await;
                            let win = window.clone();
                            window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                                tw.scheduled_animation.borrow_mut().take();
                                win.invalidate();
                            })));
                        })
                        .detach();
                    }
                }
            }
        }

        Ok(())
    }

    pub fn paint_modal(&mut self) -> anyhow::Result<()> {
        if let Some(modal) = self.get_modal() {
            for computed in modal.computed_element(self)?.iter() {
                let mut ui_items = computed.ui_items();

                let gl_state = self.render_state.as_ref().unwrap();
                self.render_element(&computed, gl_state, None)?;

                self.ui_items.append(&mut ui_items);
            }
        }

        Ok(())
    }

    pub fn paint_pass(&mut self) -> anyhow::Result<()> {
        {
            let gl_state = self.render_state.as_ref().unwrap();
            for layer in gl_state.layers.borrow().iter() {
                layer.clear_quad_allocation();
            }
        }

        // Clear out UI item positions; we'll rebuild these as we render
        self.ui_items.clear();

        let panes = self.get_panes_to_render();
        let focused = self.focused.is_some();
        let window_is_transparent =
            !self.window_background.is_empty() || self.config.window_background_opacity != 1.0;

        let start = Instant::now();
        let gl_state = self.render_state.as_ref().unwrap();
        let layer = gl_state
            .layer_for_zindex(0)
            .context("layer_for_zindex(0)")?;
        let mut layers = layer.quad_allocator();
        log::trace!("quad map elapsed {:?}", start.elapsed());
        metrics::histogram!("quad.map").record(start.elapsed());

        let mut paint_terminal_background = false;

        // Render the full window background
        match (self.window_background.is_empty(), self.allow_images) {
            (false, AllowImage::Yes | AllowImage::Scale(_)) => {
                let bg_color = self.palette().background.to_linear();

                let top = panes
                    .iter()
                    .find(|p| p.is_active)
                    .map(|p| match self.get_viewport(p.pane.pane_id()) {
                        Some(top) => top,
                        None => p.pane.get_dimensions().physical_top,
                    })
                    .unwrap_or(0);

                let loaded_any = self
                    .render_backgrounds(bg_color, top)
                    .context("render_backgrounds")?;

                if !loaded_any {
                    // Either there was a problem loading the background(s)
                    // or they haven't finished loading yet.
                    // Use the regular terminal background until that changes.
                    paint_terminal_background = true;
                }
            }
            _ if window_is_transparent => {
                // Avoid doubling up the background color for the main pane area.
                // We still need to paint strips that are intentionally excluded
                // from pane background quads.
                let strip_background = if panes.len() == 1 {
                    panes[0].pane.palette().background
                } else {
                    self.palette().background
                }
                .to_linear()
                .mul_alpha(self.config.window_background_opacity);
                let border = self.get_os_border();
                let tab_bar_height = if self.show_tab_bar {
                    self.tab_bar_pixel_height()
                        .context("tab_bar_pixel_height")?
                } else {
                    0.0
                };
                let padding_bottom =
                    self.config
                        .window_padding
                        .bottom
                        .evaluate_as_pixels(DimensionContext {
                            dpi: self.dimensions.dpi as f32,
                            pixel_max: self.terminal_size.pixel_height as f32,
                            pixel_cell: self.render_metrics.cell_size.height as f32,
                        });
                let top_fill_height = border.top.get() as f32
                    + if self.config.tab_bar_at_bottom {
                        0.0
                    } else {
                        tab_bar_height
                    };
                let bottom_fill_height = padding_bottom
                    + border.bottom.get() as f32
                    + if self.config.tab_bar_at_bottom {
                        tab_bar_height
                    } else {
                        0.0
                    };
                let right_fill_width =
                    self.effective_right_padding(&self.config) as f32 + border.right.get() as f32;
                let window_width = self.dimensions.pixel_width as f32;
                let window_height = self.dimensions.pixel_height as f32;

                if top_fill_height > 0.0 {
                    self.filled_rectangle(
                        &mut layers,
                        0,
                        euclid::rect(0.0, 0.0, window_width, top_fill_height.min(window_height)),
                        strip_background,
                    )
                    .context("filled_rectangle for transparent top strip")?;
                }

                if bottom_fill_height > 0.0 {
                    let clamped_height = bottom_fill_height.min(window_height);
                    self.filled_rectangle(
                        &mut layers,
                        0,
                        euclid::rect(
                            0.0,
                            (window_height - clamped_height).max(0.0),
                            window_width,
                            clamped_height,
                        ),
                        strip_background,
                    )
                    .context("filled_rectangle for transparent bottom strip")?;
                }

                if right_fill_width > 0.0 {
                    let clamped_width = right_fill_width.min(window_width);
                    let right_fill_y = top_fill_height.min(window_height);
                    let right_fill_height =
                        (window_height - right_fill_y - bottom_fill_height.min(window_height))
                            .max(0.0);
                    self.filled_rectangle(
                        &mut layers,
                        0,
                        euclid::rect(
                            window_width - clamped_width,
                            right_fill_y,
                            clamped_width,
                            right_fill_height,
                        ),
                        strip_background,
                    )
                    .context("filled_rectangle for transparent right strip")?;
                }
            }
            _ => {
                paint_terminal_background = true;
            }
        }

        if paint_terminal_background {
            // Regular window background color
            let background = if panes.len() == 1 {
                // If we're the only pane, use the pane's palette
                // to draw the padding background
                panes[0].pane.palette().background
            } else {
                self.palette().background
            }
            .to_linear()
            .mul_alpha(self.config.window_background_opacity);

            self.filled_rectangle(
                &mut layers,
                0,
                euclid::rect(
                    0.,
                    0.,
                    self.dimensions.pixel_width as f32,
                    self.dimensions.pixel_height as f32,
                ),
                background,
            )
            .context("filled_rectangle for window background")?;
        }

        let hide_transition_content = self
            .window
            .as_ref()
            .map(|window| window.is_zoom_animation_active())
            .unwrap_or(false);
        if hide_transition_content {
            // During fullscreen transition, keep only a stable background to avoid
            // one-frame text scale pops.
            let hide_background = self.palette().background.to_linear();
            self.filled_rectangle(
                &mut layers,
                0,
                euclid::rect(
                    0.,
                    0.,
                    self.dimensions.pixel_width as f32,
                    self.dimensions.pixel_height as f32,
                ),
                hide_background,
            )
            .context("filled_rectangle for fullscreen transition hide")?;
            drop(layers);
            return Ok(());
        }

        let num_panes = panes.len();
        let mut active_pane_top_right: Option<(f32, f32, bool)> = None;

        for pos in panes {
            if pos.is_active && num_panes > 1 {
                let cell_width = self.render_metrics.cell_size.width as f32;
                let cell_height = self.render_metrics.cell_size.height as f32;
                let (padding_left, padding_top) = self.padding_left_top();
                let border = self.get_os_border();
                let tab_bar_height = if self.show_tab_bar {
                    self.tab_bar_pixel_height().unwrap_or(0.)
                } else {
                    0.
                };
                let top_bar_height = if self.config.tab_bar_at_bottom {
                    0.0
                } else {
                    tab_bar_height
                };
                let top_pixel_y = top_bar_height + padding_top + border.top.get() as f32;

                let x = padding_left
                    + border.left.get() as f32
                    + ((pos.left + pos.width) as f32 * cell_width);
                let y = top_pixel_y + (pos.top as f32 * cell_height);
                let is_top_pane = pos.top == 0;
                active_pane_top_right = Some((x, y, is_top_pane));
            }
            if pos.is_active {
                self.update_text_cursor(&pos);
                if focused {
                    pos.pane.advise_focus();
                    mux::Mux::get().record_focus_for_current_identity(pos.pane.pane_id());
                }
                // Clear unread bell when pane becomes active (covers mouse click, tab switch, etc.)
                self.pane_state(pos.pane.pane_id()).has_unread_bell = false;
            }
            self.paint_pane(&pos, &mut layers).context("paint_pane")?;
        }

        static CIRCLE_POLY: &[Poly] = &[Poly {
            path: &[PolyCommand::Circle {
                center: (BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                radius: BlockCoord::Frac(1, 2),
            }],
            intensity: BlockAlpha::Full,
            style: PolyStyle::Fill,
        }];

        const DOT_SIZE: f32 = 10.0;
        const RIGHT_INSET: f32 = 3.0;
        const TOP_PANE_MARGIN_WITH_TAB_BAR: f32 = 24.0;
        const TOP_PANE_MARGIN_NO_TAB_BAR: f32 = 14.0;
        const LOWER_PANE_MARGIN: f32 = 20.0;

        // Draw dot indicator for the active pane when split
        if let Some((dot_x, dot_y, is_top_pane)) = active_pane_top_right {
            let top_pane_margin = if self.show_tab_bar && !self.config.tab_bar_at_bottom {
                TOP_PANE_MARGIN_WITH_TAB_BAR
            } else {
                TOP_PANE_MARGIN_NO_TAB_BAR
            };
            let margin_top = if is_top_pane {
                top_pane_margin
            } else {
                LOWER_PANE_MARGIN
            };

            const DOT_ALPHA: f32 = 0.5;
            let dot_color = self.palette().cursor_bg.to_linear().mul_alpha(DOT_ALPHA);

            self.poly_quad(
                &mut layers,
                2,
                euclid::point2(dot_x - DOT_SIZE - RIGHT_INSET, dot_y + margin_top),
                CIRCLE_POLY,
                1,
                euclid::size2(DOT_SIZE, DOT_SIZE),
                dot_color,
            )
            .context("active pane indicator")?;
        }

        if let Some(pane) = self.get_active_pane_or_overlay() {
            let splits = self.get_splits();
            for split in &splits {
                self.paint_split(&mut layers, split, &splits, &pane)
                    .context("paint_split")?;
            }
        }

        // Draw visual notification dot on inactive tabs with unread bell
        if self.show_tab_bar {
            self.paint_tab_bar(&mut layers).context("paint_tab_bar")?;
            self.paint_tab_bell_indicators(&mut layers)?;
        }

        self.paint_window_borders(&mut layers)
            .context("paint_window_borders")?;
        drop(layers);
        self.paint_modal().context("paint_modal")?;
        self.paint_toast().context("paint_toast")?;

        Ok(())
    }

    /// Draw a blinking dot on inactive tabs that have panes with unread bell events.
    fn paint_tab_bell_indicators(
        &mut self,
        layers: &mut crate::quad::TripleLayerQuadAllocator,
    ) -> anyhow::Result<()> {
        use crate::tabbar::TabBarItem;

        // Figure out which tab indices have unread bell panes
        let mux = mux::Mux::get();
        let mux_window = match mux.get_window(self.mux_window_id) {
            Some(w) => w,
            None => return Ok(()),
        };
        let active_tab_idx = mux_window.get_active_idx();

        let mut tabs_with_bell: HashSet<usize> = HashSet::new();
        for (idx, tab) in mux_window.iter().enumerate() {
            if idx == active_tab_idx {
                continue; // skip active tab
            }
            let panes = tab.iter_panes_ignoring_zoom();
            for pos in &panes {
                if self.pane_state(pos.pane.pane_id()).has_unread_bell {
                    tabs_with_bell.insert(idx);
                    break;
                }
            }
        }

        if tabs_with_bell.is_empty() {
            return Ok(());
        }

        // Blink: 500ms on, 500ms off
        let blink_phase = self.created.elapsed().as_millis() % 1000;
        let is_visible = blink_phase >= 500;

        if is_visible {
            let notif_color = self.palette().cursor_bg.to_linear().mul_alpha(0.9);

            static CIRCLE_POLY: &[Poly] = &[Poly {
                path: &[PolyCommand::Circle {
                    center: (BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    radius: BlockCoord::Frac(1, 2),
                }],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }];

            const DOT_SIZE: f32 = 10.0;
            const DOT_RIGHT_MARGIN: f32 = 6.0;

            for ui_item in &self.ui_items {
                if let crate::termwindow::UIItemType::TabBar(TabBarItem::Tab {
                    tab_idx,
                    active: false,
                }) = &ui_item.item_type
                {
                    if tabs_with_bell.contains(tab_idx) {
                        // Draw dot at the right side of the tab, vertically centered
                        let dot_x =
                            (ui_item.x + ui_item.width) as f32 - DOT_SIZE - DOT_RIGHT_MARGIN;
                        let dot_y = ui_item.y as f32 + (ui_item.height as f32 - DOT_SIZE) / 2.0;

                        self.poly_quad(
                            layers,
                            2,
                            euclid::point2(dot_x, dot_y),
                            CIRCLE_POLY,
                            1,
                            euclid::size2(DOT_SIZE, DOT_SIZE),
                            notif_color,
                        )
                        .context("tab bell indicator")?;
                    }
                }
            }
        }

        // Schedule re-render at next blink toggle (500ms cadence)
        let next = Instant::now() + Duration::from_millis(500);
        let mut anim = self.has_animation.borrow_mut();
        match *anim {
            Some(existing) if existing <= next => {}
            _ => {
                *anim = Some(next);
            }
        }

        Ok(())
    }

    /// Render the toast notification
    pub fn paint_toast(&mut self) -> anyhow::Result<()> {
        let (toast_at, message, lifetime) = match &self.toast {
            Some((t, msg, lifetime)) if t.elapsed() < *lifetime => (*t, msg.clone(), *lifetime),
            _ => return Ok(()),
        };

        let font = self.fonts.title_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());

        // Fade out during the last 500ms of the configured lifetime.
        let elapsed_ms = toast_at.elapsed().as_millis() as f32;
        let lifetime_ms = lifetime.as_millis() as f32;
        let fade_start_ms = (lifetime_ms - 500.0).max(0.0);
        let alpha = if elapsed_ms > fade_start_ms {
            (1.0 - (elapsed_ms - fade_start_ms) / 500.0).max(0.0)
        } else {
            1.0
        };

        // Use bright purple (ansi index 13) for toast background
        let palette = self.palette();
        let bg_linear = palette.colors.0[13].to_linear();
        let bg_color = LinearRgba(bg_linear.0, bg_linear.1, bg_linear.2, 0.9 * alpha);
        // Always use white text for visibility
        let text_color = LinearRgba(1.0, 1.0, 1.0, alpha);
        let toast_radius = Dimension::Pixels(8.0);

        let text = Element::new(&font, ElementContent::Text(message.clone()))
            .colors(ElementColors {
                border: BorderColor::default(),
                bg: LinearRgba::TRANSPARENT.into(),
                text: text_color.into(),
            })
            .display(DisplayType::Block);

        let element = Element::new(&font, ElementContent::Children(vec![text]))
            .colors(ElementColors {
                // Rounded corner polys use border colors even if the border
                // width is zero; match bg to avoid corner gaps.
                border: BorderColor::new(bg_color.into()),
                bg: bg_color.into(),
                text: text_color.into(),
            })
            .padding(BoxDimension {
                left: Dimension::Cells(0.75),
                right: Dimension::Cells(0.75),
                top: Dimension::Cells(0.25),
                bottom: Dimension::Cells(0.25),
            })
            .border(BoxDimension::new(Dimension::Pixels(0.0)))
            .border_corners(Some(Corners {
                top_left: SizedPoly {
                    width: toast_radius,
                    height: toast_radius,
                    poly: TOP_LEFT_ROUNDED_CORNER,
                },
                top_right: SizedPoly {
                    width: toast_radius,
                    height: toast_radius,
                    poly: TOP_RIGHT_ROUNDED_CORNER,
                },
                bottom_left: SizedPoly {
                    width: toast_radius,
                    height: toast_radius,
                    poly: BOTTOM_LEFT_ROUNDED_CORNER,
                },
                bottom_right: SizedPoly {
                    width: toast_radius,
                    height: toast_radius,
                    poly: BOTTOM_RIGHT_ROUNDED_CORNER,
                },
            }));

        let dimensions = self.dimensions;
        let border = self.get_os_border();
        // Calculate width based on message length (each char ~cell_width + padding)
        let approx_width = (message.len() as f32 + 1.5) * metrics.cell_size.width as f32;
        let toast_height = metrics.cell_size.height as f32 * 1.5;
        // Use consistent margin based on cell size
        let h_margin = metrics.cell_size.width as f32 * 2.0;
        let v_margin = metrics.cell_size.height as f32 * 2.0;

        // Position at bottom-right with fixed margin from window edge
        let right_x =
            dimensions.pixel_width as f32 - approx_width - h_margin - border.right.get() as f32;
        let bottom_y =
            dimensions.pixel_height as f32 - toast_height - v_margin - border.bottom.get() as f32;

        let computed = self.compute_element(
            &LayoutContext {
                height: DimensionContext {
                    dpi: dimensions.dpi as f32,
                    pixel_max: dimensions.pixel_height as f32,
                    pixel_cell: metrics.cell_size.height as f32,
                },
                width: DimensionContext {
                    dpi: dimensions.dpi as f32,
                    pixel_max: dimensions.pixel_width as f32,
                    pixel_cell: metrics.cell_size.width as f32,
                },
                bounds: euclid::rect(right_x, bottom_y, approx_width, toast_height),
                metrics: &metrics,
                gl_state: self.render_state.as_ref().unwrap(),
                zindex: 120,
            },
            &element,
        )?;

        let gl_state = self.render_state.as_ref().unwrap();
        self.render_element(&computed, gl_state, None)?;

        // Keep redrawing during fade-out
        if elapsed_ms > fade_start_ms {
            let next = Instant::now() + Duration::from_millis(16);
            let mut anim = self.has_animation.borrow_mut();
            match *anim {
                Some(existing) if existing <= next => {}
                _ => {
                    *anim = Some(next);
                }
            }
        }

        Ok(())
    }
}
