# Windows-Style Autoscroll Behavior

This document describes the expected behavior of Windows-style middle-click autoscroll functionality, used as a reference for implementing the feature in RazerLinux.

## Overview

Windows autoscroll (also known as "omnidirectional scrolling" or "auto-scroll") is a feature that allows users to scroll in any direction by middle-clicking and moving the mouse pointer. The scroll speed is proportional to the distance from the origin point (where the middle-click occurred).

## Two Activation Modes

Windows autoscroll supports **two distinct activation modes**:

### 1. Hold Mode (Drag-to-Scroll)
- **Activation**: Press and **hold** the middle mouse button
- **Scrolling**: Move the mouse while holding the button - scrolling occurs continuously
- **Deactivation**: Release the middle mouse button
- **Use case**: Quick scrolling through content

### 2. Toggle/Lock Mode (Click-to-Lock)
- **Activation**: **Short click** (quick press and release) the middle mouse button
- **Scrolling**: Move the mouse (button is released) - scrolling continues based on cursor position
- **Deactivation**: Any of the following:
  - Click any mouse button (left, right, or middle)
  - Press the **Esc** key
  - Click outside a scrollable area (in some implementations)
- **Use case**: Hands-free scrolling for reading long documents

## Visual Indicator (Origin Icon)

When autoscroll is activated, Windows displays a visual indicator at the origin point (where the middle-click occurred). This indicator:

### Icon Design
- **Shape**: Circle with directional arrows
- **Standard icon variants**:
  - **Vertical-only** (↑↓): Circle with up/down arrows - for content that only scrolls vertically
  - **Horizontal-only** (←→): Circle with left/right arrows - for content that only scrolls horizontally  
  - **Omnidirectional** (↑↓←→): Circle with four arrows pointing up, down, left, and right - for content that scrolls in all directions
- **Size**: Approximately 16x16 to 24x24 pixels
- **Color**: Typically black/dark with white background or semi-transparent
- **Position**: Fixed at the origin point (does not follow cursor)

### Windows Standard Autoscroll Icons

```
    Vertical Only:           Horizontal Only:         Omnidirectional:
    
         ▲                                                  ▲
         |                                                  |
       ──●──               ────●────                    ← ──●── →
         |                                                  |
         ▼                                                  ▼
```

The actual Windows icons are:
- A small circle (origin point)
- Arrows indicating available scroll directions
- Clean, minimal design
- Semi-transparent or with subtle shadow

## Scrollable Area Detection

In Windows, autoscroll:
- **Only activates** in areas that support scrolling (scrollable windows, text areas, web pages, etc.)
- **Does NOT activate** when middle-clicking on non-scrollable areas (regular buttons, desktop, etc.)
- When clicking a non-scrollable area, the regular middle-click action is performed instead

### Typical Scrollable Areas
- Web browsers (page content)
- Text editors and word processors
- PDF viewers
- File managers with scrollable views
- Any window with a scrollbar

### Non-Scrollable Areas
- Taskbar
- Desktop background
- Dialog buttons
- Menu bars (unless in a scrollable menu)

## Scroll Behavior

### Direction
- **Vertical**: Moving cursor UP from origin scrolls content DOWN (like dragging a page up)
- **Horizontal**: Moving cursor LEFT from origin scrolls content RIGHT
- The scroll direction follows "drag" convention (imagine grabbing and pulling the content)

### Speed/Acceleration
- **Dead zone**: Small area around origin (typically 5-20 pixels) where no scrolling occurs
- **Linear acceleration**: Speed increases with distance from origin
- **Speed zones** (approximate):
  - **0-10px**: No scrolling (dead zone)
  - **10-50px**: Slow scrolling
  - **50-100px**: Medium scrolling
  - **100px+**: Fast scrolling
- **Continuous**: Scrolling is smooth and continuous, not discrete "clicks"

### Cursor Behavior
- **Cursor moves freely**: The mouse cursor moves normally on screen
- The visual indicator **stays fixed** at the origin point
- Scroll direction and speed are calculated based on cursor distance from the fixed origin

## Keyboard Interaction

- **Esc key**: Cancels autoscroll mode (in toggle mode)
- Other keys may also cancel autoscroll depending on the application

## Application-Specific Behavior

Different Windows applications may implement autoscroll slightly differently:

### Web Browsers (Chrome, Firefox, Edge)
- Full omnidirectional support
- Works on scrollable web content
- May not work in input fields

### Microsoft Office
- Vertical and horizontal scrolling
- Works in document views

### File Explorer
- Vertical scrolling in file lists
- May require content to overflow visible area

## Implementation Requirements for RazerLinux

Based on the above Windows behavior, RazerLinux autoscroll should:

### Core Features
1. ✅ Support both Hold Mode and Toggle Mode
2. ✅ Show visual indicator at origin point
3. ✅ Allow free cursor movement while scrolling
4. ✅ Calculate scroll speed based on distance from origin
5. ✅ Include a dead zone near the origin
6. ✅ Provide smooth, continuous scrolling

### Visual Indicator
1. ✅ Use Windows-style icon design (circle with arrows)
2. ✅ Show appropriate directional arrows (up/down/left/right)
3. ✅ Keep indicator at fixed origin position
4. ✅ Use appropriate size (~32 pixels)
5. ✅ Use dark theme compatible colors (white on transparent)

### Activation/Deactivation
1. ✅ Hold Mode: Activate on press, deactivate on release
2. ✅ Toggle Mode: Activate on short click (<200ms), deactivate on next click
3. ✅ Any button click exits autoscroll (in toggle mode)
4. ✅ Differentiate between short click (toggle) and hold (drag)

### Implementation Details (v0.2.0)
- **Hold mode**: Press and hold middle button, scrolling occurs while held, release to exit
- **Toggle mode**: Quick click (<200ms, no movement) enters toggle mode, click any button to exit
- **Short click threshold**: 200ms - clicks shorter than this with no movement enter toggle mode
- **Dead zone**: 15 pixels from origin before scrolling starts
- **Scroll speed**: Proportional to distance, with configurable divisor
- **Icon size**: 32x32 pixels with filled triangular arrows

## References

- [Wikipedia: Scroll wheel - Omnidirectional scrolling](https://en.wikipedia.org/wiki/Scroll_wheel)
- [Wikipedia: Mouse button - Scroll wheel](https://en.wikipedia.org/wiki/Mouse_button#Scroll_wheel)
- Microsoft Windows mouse input documentation
