#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeLocale {
    ZhCn,
    En,
}

pub(crate) struct NativeCopy {
    pub(crate) show: &'static str,
    pub(crate) capture: &'static str,
    pub(crate) quit: &'static str,
    pub(crate) notification_title: &'static str,
    pub(crate) capture_done: &'static str,
    pub(crate) capture_failed: &'static str,
}

pub(crate) fn resolve_native_locale(setting: &str, system_locale: Option<&str>) -> NativeLocale {
    if setting == "zh-CN"
        || (setting == "system"
            && system_locale
                .unwrap_or_default()
                .to_ascii_lowercase()
                .starts_with("zh"))
    {
        NativeLocale::ZhCn
    } else {
        NativeLocale::En
    }
}

pub(crate) fn native_locale(setting: &str) -> NativeLocale {
    resolve_native_locale(setting, sys_locale::get_locale().as_deref())
}

pub(crate) fn native_copy(locale: NativeLocale) -> NativeCopy {
    match locale {
        NativeLocale::ZhCn => NativeCopy {
            show: "显示 PaddleDesk",
            capture: "截图识别",
            quit: "退出",
            notification_title: "PaddleDesk 截图识别",
            capture_done: "识别结果已复制到剪贴板",
            capture_failed: "截图识别失败",
        },
        NativeLocale::En => NativeCopy {
            show: "Show PaddleDesk",
            capture: "Capture and recognize",
            quit: "Quit",
            notification_title: "PaddleDesk screen recognition",
            capture_done: "Recognition result copied to the clipboard",
            capture_failed: "Screen recognition failed",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{native_copy, resolve_native_locale, NativeLocale};

    #[test]
    fn resolves_system_language_with_english_fallback() {
        assert_eq!(
            resolve_native_locale("system", Some("zh-Hans-CN")),
            NativeLocale::ZhCn
        );
        assert_eq!(
            resolve_native_locale("system", Some("fr-FR")),
            NativeLocale::En
        );
        assert_eq!(
            resolve_native_locale("zh-CN", Some("en-US")),
            NativeLocale::ZhCn
        );
    }

    #[test]
    fn native_copy_covers_tray_and_capture_notifications() {
        let zh = native_copy(NativeLocale::ZhCn);
        assert_eq!(
            (zh.show, zh.capture, zh.quit),
            ("显示 PaddleDesk", "截图识别", "退出")
        );
        assert_eq!(zh.capture_done, "识别结果已复制到剪贴板");

        let en = native_copy(NativeLocale::En);
        assert_eq!(
            (en.show, en.capture, en.quit),
            ("Show PaddleDesk", "Capture and recognize", "Quit")
        );
        assert_eq!(en.capture_failed, "Screen recognition failed");
    }
}
