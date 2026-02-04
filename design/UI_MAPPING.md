# SwiftUI to Web Core Mapping

This document maps SwiftUI reference components and patterns to their implementation in the Tauri/Svelte desktop UI for Wavry.

| SwiftUI Component | Web Equivalent | Styling Implementation |
| :--- | :--- | :--- |
| `VStack(spacing: X)` | `display: flex; flex-direction: column; gap: X` | Uses CSS flexbox with `var(--spacing-*)` |
| `HStack(spacing: X)` | `display: flex; flex-direction: row; gap: X` | Uses CSS flexbox with `var(--spacing-*)` |
| `ZStack` | `position: relative` with absolute children | Manual layering |
| `Color(red: G: B:)` | Hex or RGBA | Mapping via `--colors-*-*` tokens |
| `.cornerRadius(X)` | `border-radius: X` | Uses `var(--radius-*)` |
| `.font(.system(size: X))`| `font-size: Xpx` | Uses `var(--font-size-*)` |
| `.fontWeight(.bold)` | `font-weight: 700` | Uses `var(--font-weight-*)` |
| `withAnimation` | `transition` or `animate` | Timing: `var(--animation-duration-standard)` |
| `Spacer()` | `flex: 1` | Standard flex-grow behavior |

## Component Mapping

| Wavry Component | SwiftUI File | Svelte File | Notes |
| :--- | :--- | :--- | :--- |
| **App Layout** | `ContentView.swift` | `+page.svelte` | Mirrored Sidebar+Content layout. |
| **Sidebar Icon** | `SidebarIcon` (struct) | `SidebarIcon.svelte` | Mirrored hover/active states and radii. |
| **Host Card** | `HostCard` (struct) | `HostCard.svelte` | Mirrored preview height, overlay, and info row. |
| **Input Fields** | `TextField` | `input` (global styles) | Mirrored padding and background. |
| **Primary Buttons**| `Button` | `action-btn` (class) | Mirrored vertical/horizontal padding. |

## Animation Specs

| Animation | Timing Curve | Duration |
| :--- | :--- | :--- |
| **Standard Transition** | `easeInOut` (standard) | 350ms |
| **Token Variable** | `var(--animation-easing-standard)` | `var(--animation-duration-standard)` |
