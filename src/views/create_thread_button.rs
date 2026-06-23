use std::time::Duration;

use gpui::{
    Animation, AnimationExt, BoxShadow, Context, EventEmitter, FontWeight, Hsla, IntoElement,
    ParentElement, Render, Styled, Window, div, linear_color_stop, linear_gradient, point, prelude::*,
    px, rgb, rgba, svg,
};

#[derive(Clone)]
pub enum CreateThreadButtonEvent {
    Clicked,
}

pub struct CreateThreadButton {
    hovered: bool,
    pressed: bool,
}

impl CreateThreadButton {
    pub fn new() -> Self {
        Self {
            hovered: false,
            pressed: false,
        }
    }

    fn glow_shadow(alpha: f32, blur: f32, spread: f32) -> BoxShadow {
        BoxShadow {
            color: Into::<Hsla>::into(rgb(0x6366f1)).alpha(alpha),
            offset: point(px(0.), px(0.)),
            blur_radius: px(blur),
            spread_radius: px(spread),
        }
    }

    fn cyan_glow(alpha: f32, blur: f32) -> BoxShadow {
        BoxShadow {
            color: Into::<Hsla>::into(rgb(0x22d3ee)).alpha(alpha),
            offset: point(px(0.), px(0.)),
            blur_radius: px(blur),
            spread_radius: px(0.),
        }
    }

    fn breath_overlay(hovered: bool) -> impl IntoElement {
        let base_alpha = if hovered { 0.12 } else { 0.06 };
        let peak_alpha = if hovered { 0.28 } else { 0.14 };
        let base_blur = if hovered { 18. } else { 12. };
        let peak_blur = if hovered { 32. } else { 22. };

        div()
            .id("create-thread-breath-overlay")
            .absolute()
            .top(px(0.))
            .right(px(0.))
            .bottom(px(0.))
            .left(px(0.))
            .rounded_full()
            .bg(linear_gradient(
                180.0,
                linear_color_stop(rgba(0xffffff44), 0.),
                linear_color_stop(rgba(0x22d3ee33), 1.),
            ))
            .with_animation(
                "create-thread-breath",
                Animation::new(Duration::from_millis(2000)).repeat(),
                move |overlay, progress| {
                    let wave = (progress * std::f32::consts::PI * 2.0).sin();
                    let breath = 0.5 + 0.5 * wave;
                    let alpha = base_alpha + breath * (peak_alpha - base_alpha);
                    let blur = base_blur + breath * (peak_blur - base_blur);
                    overlay.opacity(alpha).shadow(vec![BoxShadow {
                        color: Into::<Hsla>::into(rgb(0x22d3ee)).alpha(alpha * 0.9),
                        offset: point(px(0.), px(0.)),
                        blur_radius: px(blur),
                        spread_radius: px(0.),
                    }])
                },
            )
    }

}

impl EventEmitter<CreateThreadButtonEvent> for CreateThreadButton {}

impl Render for CreateThreadButton {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let hovered = self.hovered;
        let pressed = self.pressed;

        let base_gradient = linear_gradient(
            180.0,
            linear_color_stop(rgb(0x8b5cf6), 0.),
            linear_color_stop(rgb(0x4f46e5), 1.),
        );

        let hover_gradient = linear_gradient(
            180.0,
            linear_color_stop(rgb(0xa78bfa), 0.),
            linear_color_stop(rgb(0x6366f1), 1.),
        );

        let active_gradient = linear_gradient(
            180.0,
            linear_color_stop(rgb(0x4f46e5), 0.),
            linear_color_stop(rgb(0x4338ca), 1.),
        );

        let current_bg = if pressed {
            active_gradient
        } else if hovered {
            hover_gradient
        } else {
            base_gradient
        };

        let base_shadows = vec![
            BoxShadow {
                color: Into::<Hsla>::into(rgb(0x000000)).alpha(0.25),
                offset: point(px(0.), px(4.)),
                blur_radius: px(10.),
                spread_radius: px(0.),
            },
            Self::glow_shadow(0.35, 16., 0.),
        ];

        let hover_shadows = vec![
            BoxShadow {
                color: Into::<Hsla>::into(rgb(0x000000)).alpha(0.3),
                offset: point(px(0.), px(6.)),
                blur_radius: px(14.),
                spread_radius: px(0.),
            },
            Self::glow_shadow(0.55, 24., 0.),
            Self::cyan_glow(0.28, 32.),
        ];

        let active_shadows = vec![
            BoxShadow {
                color: Into::<Hsla>::into(rgb(0x000000)).alpha(0.35),
                offset: point(px(0.), px(2.)),
                blur_radius: px(6.),
                spread_radius: px(0.),
            },
            Self::glow_shadow(0.25, 10., 0.),
        ];

        let current_shadows = if pressed {
            active_shadows
        } else if hovered {
            hover_shadows
        } else {
            base_shadows
        };

        div()
            .id("create-thread-btn")
            .relative()
            .flex()
            .items_center()
            .justify_center()
            .px_8()
            .py_3()
            .bg(current_bg)
            .rounded_full()
            .overflow_hidden()
            .text_color(rgb(0xffffff))
            .cursor_pointer()
            .text_base()
            .shadow(current_shadows)
            .gap(px(8.))
            .child(Self::breath_overlay(hovered))
            .child(
                div()
                    .relative()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(22.))
                    .rounded_full()
                    .bg(rgba(0xffffff22))
                    .border_1()
                    .border_color(rgba(0xffffff33))
                    .child(
                        svg()
                            .path("logo.svg")
                            .text_color(rgb(0xffffff))
                            .size(px(14.)),
                    ),
            )
            .child(
                div()
                    .relative()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Create Thread"),
            )
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if this.hovered != *hovered {
                    this.hovered = *hovered;
                    cx.notify();
                }
            }))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.pressed = true;
                    cx.notify();
                }),
            )
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.pressed = false;
                    cx.notify();
                }),
            )
            .on_click(cx.listener(|_, _, _, cx| {
                cx.emit(CreateThreadButtonEvent::Clicked);
            }))
    }
}
