use std::collections::HashMap;

use anyhow::Result;

use crate::InputInjector;

/// A single key remapping rule.
#[derive(Debug, Clone)]
pub struct KeyRemap {
    /// Source keycode (as received from the client).
    pub from: u32,
    /// Target keycode to inject, or `None` to block the key.
    pub to: Option<u32>,
}

/// A single gamepad button remapping rule.
#[derive(Debug, Clone)]
pub struct ButtonRemap {
    /// Source button index.
    pub from: u32,
    /// Target button index, or `None` to block the button.
    pub to: Option<u32>,
}

/// A named input mapping profile that remaps or blocks input events before
/// they reach the platform injector.
///
/// # Example
/// ```rust
/// use wavry_platform::InputMap;
///
/// let mut map = InputMap::new("swap-ctrl-alt");
/// // Remap Left Ctrl (29) → Left Alt (56) and vice-versa
/// map.remap_key(29, Some(56));
/// map.remap_key(56, Some(29));
/// // Block the Windows key (125)
/// map.remap_key(125, None);
/// ```
#[derive(Debug, Clone, Default)]
pub struct InputMap {
    /// Human-readable profile name.
    pub name: String,
    /// Key remapping table: `keycode → target` (None = block).
    key_map: HashMap<u32, Option<u32>>,
    /// Gamepad button remapping table: `button → target` (None = block).
    button_map: HashMap<u32, Option<u32>>,
}

impl InputMap {
    /// Create a new empty mapping with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            key_map: HashMap::new(),
            button_map: HashMap::new(),
        }
    }

    /// Add or update a key remapping rule.
    /// `to = None` blocks the key entirely.
    pub fn remap_key(&mut self, from: u32, to: Option<u32>) {
        self.key_map.insert(from, to);
    }

    /// Add or update a gamepad button remapping rule.
    /// `to = None` blocks the button entirely.
    pub fn remap_button(&mut self, from: u32, to: Option<u32>) {
        self.button_map.insert(from, to);
    }

    /// Resolve a keycode through the map. Returns `None` if the key is blocked.
    pub fn resolve_key(&self, keycode: u32) -> Option<u32> {
        match self.key_map.get(&keycode) {
            Some(mapped) => *mapped,
            None => Some(keycode),
        }
    }

    /// Resolve a gamepad button through the map. Returns `None` if blocked.
    pub fn resolve_button(&self, button: u32) -> Option<u32> {
        match self.button_map.get(&button) {
            Some(mapped) => *mapped,
            None => Some(button),
        }
    }

    /// Returns true if this map has no rules (pass-through).
    pub fn is_empty(&self) -> bool {
        self.key_map.is_empty() && self.button_map.is_empty()
    }
}

/// Wraps any [`InputInjector`] and applies an [`InputMap`] before forwarding
/// events. Use this to apply remapping without modifying the underlying
/// platform injector.
pub struct MappedInjector<I: InputInjector> {
    inner: I,
    map: InputMap,
}

impl<I: InputInjector> MappedInjector<I> {
    /// Create a new mapped injector wrapping `inner` with the given `map`.
    pub fn new(inner: I, map: InputMap) -> Self {
        Self { inner, map }
    }

    /// Replace the active mapping profile at runtime.
    pub fn set_map(&mut self, map: InputMap) {
        self.map = map;
    }

    /// Access the current mapping profile.
    pub fn map(&self) -> &InputMap {
        &self.map
    }
}

impl<I: InputInjector> InputInjector for MappedInjector<I> {
    fn key(&mut self, keycode: u32, pressed: bool) -> Result<()> {
        if let Some(mapped) = self.map.resolve_key(keycode) {
            self.inner.key(mapped, pressed)?;
        }
        Ok(())
    }

    fn mouse_button(&mut self, button: u8, pressed: bool) -> Result<()> {
        self.inner.mouse_button(button, pressed)
    }

    fn mouse_motion(&mut self, dx: i32, dy: i32) -> Result<()> {
        self.inner.mouse_motion(dx, dy)
    }

    fn mouse_absolute(&mut self, x: f32, y: f32) -> Result<()> {
        self.inner.mouse_absolute(x, y)
    }

    fn scroll(&mut self, dx: f32, dy: f32) -> Result<()> {
        self.inner.scroll(dx, dy)
    }

    fn gamepad(
        &mut self,
        gamepad_id: u32,
        axes: &[(u32, f32)],
        buttons: &[(u32, bool)],
    ) -> Result<()> {
        if self.map.button_map.is_empty() {
            return self.inner.gamepad(gamepad_id, axes, buttons);
        }
        let mapped_buttons: Vec<(u32, bool)> = buttons
            .iter()
            .filter_map(|&(btn, pressed)| self.map.resolve_button(btn).map(|b| (b, pressed)))
            .collect();
        self.inner.gamepad(gamepad_id, axes, &mapped_buttons)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockInjector {
        keys: Vec<(u32, bool)>,
        buttons: Vec<(u32, bool)>,
    }

    impl MockInjector {
        fn new() -> Self {
            Self {
                keys: vec![],
                buttons: vec![],
            }
        }
    }

    impl InputInjector for MockInjector {
        fn key(&mut self, keycode: u32, pressed: bool) -> Result<()> {
            self.keys.push((keycode, pressed));
            Ok(())
        }
        fn mouse_button(&mut self, _b: u8, _p: bool) -> Result<()> {
            Ok(())
        }
        fn mouse_motion(&mut self, _dx: i32, _dy: i32) -> Result<()> {
            Ok(())
        }
        fn mouse_absolute(&mut self, _x: f32, _y: f32) -> Result<()> {
            Ok(())
        }
        fn scroll(&mut self, _dx: f32, _dy: f32) -> Result<()> {
            Ok(())
        }
        fn gamepad(
            &mut self,
            _id: u32,
            _axes: &[(u32, f32)],
            buttons: &[(u32, bool)],
        ) -> Result<()> {
            for &(btn, pressed) in buttons {
                self.buttons.push((btn, pressed));
            }
            Ok(())
        }
    }

    #[test]
    fn passthrough_when_empty() {
        let mut map = InputMap::new("empty");
        assert!(map.is_empty());
        map.remap_key(1, Some(1));
        assert!(!map.is_empty());
    }

    #[test]
    fn key_remap_applies() {
        let mut map = InputMap::new("test");
        map.remap_key(29, Some(56)); // Ctrl → Alt
        assert_eq!(map.resolve_key(29), Some(56));
        assert_eq!(map.resolve_key(30), Some(30)); // unmapped passes through
    }

    #[test]
    fn key_block_works() {
        let mut map = InputMap::new("test");
        map.remap_key(125, None); // block Win key
        assert_eq!(map.resolve_key(125), None);
        assert_eq!(map.resolve_key(1), Some(1));
    }

    #[test]
    fn mapped_injector_remaps_keys() {
        let mut map = InputMap::new("swap");
        map.remap_key(1, Some(2));
        map.remap_key(3, None); // blocked

        let mock = MockInjector::new();
        let mut injector = MappedInjector::new(mock, map);

        injector.key(1, true).unwrap(); // 1 → 2
        injector.key(3, true).unwrap(); // blocked, nothing injected
        injector.key(5, true).unwrap(); // passthrough

        assert_eq!(injector.inner.keys, vec![(2, true), (5, true)]);
    }

    #[test]
    fn mapped_injector_remaps_gamepad_buttons() {
        let mut map = InputMap::new("gamepad");
        map.remap_button(0, Some(1)); // A → B
        map.remap_button(2, None); // X blocked

        let mock = MockInjector::new();
        let mut injector = MappedInjector::new(mock, map);

        injector
            .gamepad(0, &[], &[(0, true), (1, true), (2, true)])
            .unwrap();
        // button 0→1, button 1 pass-through, button 2 blocked
        assert_eq!(injector.inner.buttons, vec![(1, true), (1, true)]);
    }
}
