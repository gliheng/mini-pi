use gpui::{
    Context, EventEmitter, InteractiveElement, IntoElement, MouseButton, ParentElement, Render,
    SharedString, StatefulInteractiveElement, Styled, Window, div, px, rgb,
};
use gpui_component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_component::{Icon, Sizable as _, Size};

use crate::data::store::WorkspaceMeta;

#[derive(Clone)]
pub enum WorkspaceManagerEvent {
    AddRequested,
    CloseRequested,
    DeleteRequested { workspace_id: String },
}

pub struct WorkspaceManager {
    workspaces: Vec<WorkspaceMeta>,
}

impl WorkspaceManager {
    pub fn new(workspaces: Vec<WorkspaceMeta>) -> Self {
        Self { workspaces }
    }

    pub fn set_workspaces(&mut self, workspaces: Vec<WorkspaceMeta>) {
        self.workspaces = workspaces;
    }
}

impl EventEmitter<WorkspaceManagerEvent> for WorkspaceManager {}

impl Render for WorkspaceManager {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .absolute()
            .top(px(0.))
            .left(px(0.))
            .w_full()
            .h_full()
            .child(
                div()
                    .absolute()
                    .top(px(0.))
                    .left(px(0.))
                    .w_full()
                    .h_full()
                    .bg(gpui::hsla(0.0, 0.0, 0.0, 0.65))
                    .on_mouse_down(MouseButton::Left, cx.listener(|_this, _, _window, cx| {
                        cx.emit(WorkspaceManagerEvent::CloseRequested);
                    })),
            )
            .child(
                div()
                    .absolute()
                    .top(px(0.))
                    .left(px(0.))
                    .w_full()
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .child(
                        div()
                            .w(px(420.))
                            .max_h(px(400.))
                            .bg(rgb(0x1e1e1e))
                            .rounded_lg()
                            .border_1()
                            .border_color(rgb(0x333333))
                            .flex()
                            .flex_col()
                            .shadow(vec![gpui::BoxShadow {
                                color: gpui::rgba(0x000000aa).into(),
                                offset: gpui::point(px(0.), px(4.)),
                                blur_radius: px(12.),
                                spread_radius: px(0.),
                            }])
                            .child(
                                div()
                                    .px_4()
                                    .py_3()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .justify_between()
                                    .border_b_1()
                                    .border_color(rgb(0x333333))
                                    .child(
                                        div()
                                            .flex()
                                            .flex_row()
                                            .items_center()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                                    .text_color(rgb(0xe5e5e5))
                                                    .child("Workspaces"),
                                            )
                                            .child(
                                                Button::new("modal-add-workspace-btn")
                                                    .label("+")
                                                    .with_size(Size::XSmall)
                                                    .custom(
                                                        ButtonCustomVariant::new(cx)
                                                            .color(rgb(0x333333).into())
                                                            .foreground(rgb(0xcccccc).into())
                                                            .hover(rgb(0x444444).into())
                                                            .active(rgb(0x555555).into()),
                                                    )
                                                    .on_click(cx.listener(|_this, _, _window, cx| {
                                                        cx.emit(WorkspaceManagerEvent::AddRequested);
                                                    })),
                                            ),
                                    )
                                    .child(
                                        Button::new("modal-close-btn")
                                            .with_size(Size::XSmall)
                                            .custom(
                                                ButtonCustomVariant::new(cx)
                                                    .color(gpui::rgba(0x00000000).into())
                                                    .foreground(rgb(0x888888).into())
                                                    .hover(rgb(0x333333).into())
                                                    .active(rgb(0x444444).into()),
                                            )
                                            .icon(
                                                Icon::empty()
                                                    .path("close.svg")
                                                    .size(px(14.))
                                                    .text_color(rgb(0x888888)),
                                            )
                                            .on_click(cx.listener(|_this, _, _window, cx| {
                                                cx.emit(WorkspaceManagerEvent::CloseRequested);
                                            })),
                                    ),
                            )
                            .child({
                                let filtered: Vec<_> = self.workspaces.iter().filter(|ws| ws.name != "Default").collect();
                                if filtered.is_empty() {
                                    div()
                                        .id("workspace-modal-list")
                                        .flex()
                                        .flex_col()
                                        .items_center()
                                        .justify_center()
                                        .gap_2()
                                        .p_3()
                                        .h(px(120.))
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(rgb(0x666666))
                                                .child("No workspaces yet"),
                                        )
                                        .child(
                                            Button::new("modal-add-workspace-btn-empty")
                                                .label("+ Add Workspace")
                                                .with_size(Size::Small)
                                                .custom(
                                                    ButtonCustomVariant::new(cx)
                                                        .color(rgb(0x333333).into())
                                                        .foreground(rgb(0xcccccc).into())
                                                        .hover(rgb(0x444444).into())
                                                        .active(rgb(0x555555).into()),
                                                )
                                                .on_click(cx.listener(|_this, _, _window, cx| {
                                                    cx.emit(WorkspaceManagerEvent::AddRequested);
                                                })),
                                        )
                                } else {
                                    div()
                                        .id("workspace-modal-list")
                                        .flex()
                                        .flex_col()
                                        .gap_1()
                                        .p_3()
                                        .overflow_y_scroll()
                                        .children(filtered.into_iter().map(|ws| {
                                            let ws_id = ws.id.clone();
                                            div()
                                                .id(SharedString::from(format!("ws-modal-{ws_id}")))
                                                .flex()
                                                .flex_row()
                                                .items_center()
                                                .gap_2()
                                                .px_3()
                                                .py_2()
                                                .rounded_md()
                                                .bg(rgb(0x252525))
                                                .hover(|style| style.bg(rgb(0x2a2a2a)))
                                                .child(
                                                    div()
                                                        .flex()
                                                        .flex_col()
                                                        .flex_1()
                                                        .gap_0p5()
                                                        .child(
                                                            div()
                                                                .text_xs()
                                                                .text_color(rgb(0xcccccc))
                                                                .child(ws.name.clone()),
                                                        )
                                                        .child(
                                                            div()
                                                                .text_xs()
                                                                .text_color(rgb(0x666666))
                                                                .child(ws.path.clone()),
                                                        ),
                                                )
                                                .child(
                                                    Button::new(SharedString::from(format!("ws-delete-{ws_id}")))
                                                        .with_size(Size::XSmall)
                                                        .custom(
                                                            ButtonCustomVariant::new(cx)
                                                                .color(gpui::rgba(0x00000000).into())
                                                                .foreground(rgb(0x888888).into())
                                                                .hover(rgb(0x333333).into())
                                                                .active(rgb(0x444444).into()),
                                                        )
                                                        .icon(
                                                            Icon::empty()
                                                                .path("close.svg")
                                                                .size(px(12.))
                                                                .text_color(rgb(0x888888)),
                                                        )
                                                        .on_click(cx.listener(move |_this, _, _window, cx| {
                                                            cx.emit(WorkspaceManagerEvent::DeleteRequested {
                                                                workspace_id: ws_id.clone(),
                                                            });
                                                        })),
                                                )
                                        }))
                                }
                            }),
                    ),
            )
    }
}
