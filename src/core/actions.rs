use gpui::actions;

actions!(
    mini_pi,
    [
        CloseWindow,
        Quit,
        SendMessage,
        Login,
        Logout,
        SignUp,
        ConfirmInlineEdit,
        CancelInlineEdit,
        StopStreaming,
        ShowMainWindow,
        About,
        OpenInstallExtensionWindow,
        SelectFontSmall,
        SelectFontMedium,
        SelectFontLarge,
    ]
);
