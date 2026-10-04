impl crate::Context {
    /// Full-game Android flags, with touchscreen input and Pocket screen variants.
    pub fn android() -> Self {
        Self::retail(false)
            .with_flag("desktop_screen", false)
            .with_flag("pocket_screen", true)
            .with_flag("touch", true)
            .with_flag("mouse", false)
            .with_flag("is_desktop", false)
            .with_flag("win10_edition", false)
            .with_flag("microsoft_os", false)
            .with_flag("ms_platform", false)
            .with_flag("google_os", true)
            .with_flag("is_android", true)
            .with_flag("pocket_edition", true)
            .with_flag("can_quit", false)
    }
}
