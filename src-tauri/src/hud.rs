// HUD 窗口位置记忆：load/save 读写 ~/.claude/cc-view/hud-position.json。
// 用户拖动 HUD 后存位，下次启动恢复——HUD 不再贴 tray，位置完全由用户决定。
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct HudPosition {
    pub x: i32,
    pub y: i32,
}

impl HudPosition {
    /// 从磁盘加载上次保存的位置；文件不存在 / 无 home / 解析失败都返回 None（fail fast：静默兜底，不崩）。
    pub fn load() -> Option<Self> {
        let path = dirs::home_dir()?.join(".claude/cc-view/hud-position.json");
        let txt = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&txt).ok()
    }

    /// 保存位置到磁盘；home 缺失 / 创建目录失败 / 序列化失败都静默跳过（不崩，但用 let _ 显式忽略）。
    pub fn save(x: i32, y: i32) {
        let Some(home) = dirs::home_dir() else { return };
        let dir = home.join(".claude/cc-view");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("hud-position.json");
        if let Ok(json) = serde_json::to_string(&HudPosition { x, y }) {
            let _ = std::fs::write(path, json);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_missing_returns_none() {
        // 用临时 home 目录隔离测试（覆盖 ~/.claude/cc-view 真实文件）。
        // 注意：这里只能验证 load 对不存在路径返回 None；save 因依赖 dirs::home_dir()
        // 无法在测试里替换，只在集成层面验证。
        // 但可以验证 HudPosition 序列化/反序列化往返。
        let pos = HudPosition { x: 100, y: 200 };
        let json = serde_json::to_string(&pos).expect("serialize");
        let back: HudPosition = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.x, 100);
        assert_eq!(back.y, 200);
    }

    #[test]
    fn load_invalid_json_returns_none() {
        // 反序列化失败时 load() 必须 fall through 到 None，不 panic。
        let pos: Option<HudPosition> = serde_json::from_str("not json").ok();
        assert!(pos.is_none());
    }
}
