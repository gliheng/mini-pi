use gpui::{
    App, ClickEvent, Context, EventEmitter, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, Window, div, px, rgb,
};
use gpui_component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_component::{Icon, Sizable as _, Size, WindowExt as _};

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

    pub fn render_dialog_content(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let entity = cx.entity();
        let filtered: Vec<_> = self
            .workspaces
            .iter()
            .filter(|ws| ws.name != "Default")
            .collect();

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
                        .on_click({
                            let entity = entity.clone();
                            move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                                window.close_dialog(cx);
                                entity.update(cx, |_this, cx| {
                                    cx.emit(WorkspaceManagerEvent::AddRequested);
                                });
                            }
                        }),
                )
                .into_any_element()
        } else {
            div()
                .id("workspace-modal-list")
                .flex()
                .flex_col()
                .gap_1()
                .p_3()
                .overflow_y_scroll()
                .children(filtered.into_iter().map(move |ws| {
                    let ws_id = ws.id.clone();
                    let ws_id_for_delete = ws.id.clone();
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
                                        .path("icons/close.svg")
                                        .size(px(12.))
                                        .text_color(rgb(0x888888)),
                                )
                                .on_click({
                                    let entity = entity.clone();
                                    move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                                        window.close_dialog(cx);
                                        entity.update(cx, |_this, cx| {
                                            cx.emit(WorkspaceManagerEvent::DeleteRequested {
                                                workspace_id: ws_id_for_delete.clone(),
                                            });
                                        });
                                    }
                                }),
                        )
                }))
                .into_any_element()
        }
    }
}

impl EventEmitter<WorkspaceManagerEvent> for WorkspaceManager {}
