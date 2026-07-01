use std::collections::HashMap;

use gpui::{
    App, Bounds, Context, IntoElement, Render, ScrollHandle, Window, WindowBounds,
    WindowDecorations, WindowOptions, div, prelude::*, px, size,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputState};
use gpui_component::scroll::Scrollbar;
use gpui_component::{ActiveTheme, Disableable as _, Root, Sizable as _, Size, TitleBar};

use crate::config::model_config;
use crate::core::app::AppStore;
use crate::rpc::pi_rpc::BridgeProvider;

/// A standalone window for managing pi agent settings. It is organized into
/// sections; the first (and currently only) section is provider **API Keys**.
pub struct PiSettings {
    providers: Vec<BridgeProvider>,
    providers_loading: bool,
    provider_inputs: HashMap<String, gpui::Entity<InputState>>,
    _provider_input_subs: Vec<gpui::Subscription>,
    scroll_handle: ScrollHandle,
}

impl PiSettings {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut view = Self {
            providers: Vec::new(),
            providers_loading: false,
            provider_inputs: HashMap::new(),
            _provider_input_subs: Vec::new(),
            scroll_handle: ScrollHandle::new(),
        };
        view.load_providers(cx);
        view
    }

    fn load_providers(&mut self, cx: &mut Context<Self>) {
        self.providers_loading = true;
        cx.notify();

        let bridge = cx.global::<AppStore>().pi_bridge.clone();
        let weak = cx.entity().downgrade();
        cx.spawn(async move |_, cx| {
            let result = smol::unblock(move || bridge.as_ref().map(|b| b.get_providers())).await;

            let _ = weak.update(cx, |this, cx| {
                this.providers_loading = false;
                match result {
                    Some(Ok(providers)) => {
                        this.providers = providers;
                    }
                    Some(Err(e)) => {
                        eprintln!("[pi-settings] failed to load providers: {}", e);
                    }
                    None => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn save_provider_key(&mut self, provider_id: &str, cx: &mut Context<Self>) {
        let bridge = cx.global::<AppStore>().pi_bridge.clone();
        let provider_id = provider_id.to_string();
        let key = self
            .provider_inputs
            .get(&provider_id)
            .map(|input| input.read(cx).value().to_string())
            .unwrap_or_default();

        let weak = cx.entity().downgrade();
        cx.spawn(async move |_, cx| {
            let result =
                smol::unblock(move || bridge.as_ref().map(|b| b.set_auth(&provider_id, &key))).await;

            let _ = weak.update(cx, |this, cx| {
                match result {
                    Some(Ok(())) => {
                        // Refresh the provider list (Configured status) and the
                        // global model list, since available models change when
                        // a provider key is added or cleared.
                        this.load_providers(cx);
                        this.reload_global_models(cx);
                    }
                    Some(Err(e)) => {
                        eprintln!("[pi-settings] failed to save provider key: {}", e);
                    }
                    None => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Re-fetch the available model list from the bridge and publish it into
    /// `AppStore.models`. Open chat inputs observe that global and rebuild their
    /// model dropdowns in response.
    fn reload_global_models(&self, cx: &mut Context<Self>) {
        let bridge = cx.global::<AppStore>().pi_bridge.clone();
        cx.spawn(async move |_, cx| {
            let result =
                smol::unblock(move || bridge.as_ref().map(|b| model_config::load_models(b))).await;
            let _ = cx.update_global(|app: &mut AppStore, _| match result {
                Some(Ok(models)) => app.models = models,
                Some(Err(e)) => eprintln!("[pi-settings] failed to reload models: {}", e),
                None => {}
            });
        })
        .detach();
    }
}

impl Render for PiSettings {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Ensure every provider has an input field.
        for provider in &self.providers {
            if !self.provider_inputs.contains_key(&provider.id) {
                let input =
                    cx.new(|cx| InputState::new(window, cx).placeholder("API Key").masked(true));
                let _sub = cx.observe(&input, |_, _, cx| {
                    cx.notify();
                });
                self.provider_inputs.insert(provider.id.clone(), input);
                self._provider_input_subs.push(_sub);
            }
        }

        let theme = cx.theme().clone();

        let dialog_layer = Root::render_dialog_layer(window, cx);
        let notification_layer = Root::render_notification_layer(window, cx);
        let sheet_layer = Root::render_sheet_layer(window, cx);

        let content = div()
            .id("pi-settings-content")
            .flex_1()
            .overflow_y_scroll()
            .track_scroll(&self.scroll_handle)
            .flex()
            .flex_col()
            .gap_6()
            .px_6()
            .py_6()
            .child(render_api_keys_section(self, cx));

        div()
            .id("pi-settings")
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.background)
            .text_color(theme.foreground)
            .font_family(theme.font_family.clone())
            .child(
                TitleBar::new().child(
                    div().flex().flex_row().items_center().px_2().child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("Pi Settings"),
                    ),
                ),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .min_h(px(0.))
                    .child(content)
                    .child(
                        div()
                            .absolute()
                            .top(px(0.))
                            .right(px(0.))
                            .bottom(px(0.))
                            .w(px(12.))
                            .child(Scrollbar::vertical(&self.scroll_handle)),
                    ),
            )
            .children(dialog_layer)
            .children(notification_layer)
            .children(sheet_layer)
    }
}

fn render_api_keys_section(view: &mut PiSettings, cx: &mut Context<PiSettings>) -> impl IntoElement {
    let mut section = div()
        .w_full()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .px_2()
                .py_1()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child("API KEYS"),
        )
        .child(
            div()
                .px_2()
                .pb_1()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child("API keys for the model providers available to the pi agent."),
        );

    if view.providers_loading {
        section = section.child(
            div()
                .w_full()
                .px_4()
                .py_2()
                .rounded_lg()
                .bg(cx.theme().secondary)
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("Loading providers..."),
                ),
        );
    } else if view.providers.is_empty() {
        section = section.child(
            div()
                .w_full()
                .px_4()
                .py_2()
                .rounded_lg()
                .bg(cx.theme().secondary)
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("No providers available."),
                ),
        );
    } else {
        for provider in &view.providers {
            let provider_id = provider.id.clone();
            let is_configured = provider.configured;
            let input_entity = view.provider_inputs.get(&provider_id).cloned();

            let mut row = div()
                .id(format!("provider-{}", provider_id))
                .w_full()
                .flex()
                .flex_col()
                .gap_2()
                .px_4()
                .py_3()
                .rounded_lg()
                .bg(cx.theme().secondary)
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .text_sm()
                                .text_color(cx.theme().foreground)
                                .child(provider.name.clone()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(if is_configured {
                                    cx.theme().success
                                } else {
                                    cx.theme().muted_foreground
                                })
                                .child(if is_configured {
                                    "Configured"
                                } else {
                                    "Not configured"
                                }),
                        ),
                );

            if let Some(input) = input_entity {
                let input_val = input.read(cx).value().to_string();
                let provider_id_for_save = provider_id.clone();
                let provider_id_for_clear = provider_id.clone();

                row = row.child(
                    div()
                        .w_full()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(div().flex_1().child(Input::new(&input).appearance(false).w_full()))
                        .child(
                            Button::new(format!("save-provider-{}", provider_id_for_save))
                                .label("Save")
                                .with_size(Size::Small)
                                .primary()
                                .disabled(input_val.is_empty())
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.save_provider_key(&provider_id_for_save, cx);
                                })),
                        )
                        .when(is_configured, |this| {
                            this.child(
                                Button::new(format!("clear-provider-{}", provider_id_for_clear))
                                    .label("Clear")
                                    .with_size(Size::Small)
                                    .danger()
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        if let Some(input) =
                                            this.provider_inputs.get(&provider_id_for_clear)
                                        {
                                            input.update(cx, |input, cx| {
                                                input.set_value("", window, cx);
                                            });
                                        }
                                        this.save_provider_key(&provider_id_for_clear, cx);
                                    })),
                            )
                        }),
                );
            }

            section = section.child(row);
        }
    }

    section
}

pub fn open_pi_settings_window(cx: &mut App) {
    // Singleton: focus the existing window instead of opening a duplicate.
    let handle = cx.update_global::<AppStore, _>(|app, _| app.pi_settings_window);
    if let Some(handle) = handle {
        let activated = handle
            .update(cx, |_view, window, _app| {
                window.activate_window();
            })
            .is_ok();
        if activated {
            return;
        }
    }

    let width = px(560.0);
    let height = px(520.0);
    let bounds = Bounds::centered(None, size(width, height), cx);
    let window_options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitleBar::title_bar_options()),
        window_decorations: if cfg!(target_os = "macos") {
            None
        } else {
            Some(WindowDecorations::Client)
        },
        ..Default::default()
    };

    let handle = cx
        .open_window(window_options, |window, cx| {
            let view = cx.new(|cx| PiSettings::new(window, cx));
            cx.new(|cx| Root::new(view, window, cx))
        })
        .expect("failed to open the pi settings window");

    cx.update_global::<AppStore, _>(|app, _| {
        app.pi_settings_window = Some(handle.into());
    });
}
