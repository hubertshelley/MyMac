//! 全局快捷键 F1（global-hotkey）。必须在主线程创建 manager。

use global_hotkey::hotkey::{Code, HotKey};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

use crate::event::{UiEvent, UiSink};

pub fn setup() -> global_hotkey::Result<GlobalHotKeyManager> {
    let manager = GlobalHotKeyManager::new()?;
    manager.register(HotKey::new(None, Code::F1))?;
    Ok(manager)
}

pub fn spawn_forwarder(sink: UiSink) {
    std::thread::spawn(move || {
        while let Ok(event) = GlobalHotKeyEvent::receiver().recv() {
            if event.state() == HotKeyState::Pressed {
                sink.send(UiEvent::StartScreenshot);
            }
        }
    });
}
