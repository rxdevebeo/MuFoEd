# Application icon

Supplied by the owner in `mascot_app_icons.zip`. Files are used unchanged.

- `mascot_icon.ico`: Windows executable resource with multiple icon sizes.
- `mascot_icon_256x256.png`: native window/task-switcher icon.
- `mascot_icon_32x32.png`: application mark in the tab strip.

PNG artwork is compiled into the application on every platform. Windows resources
are compiled by `winresource` at build time; there is no runtime tool dependency.
