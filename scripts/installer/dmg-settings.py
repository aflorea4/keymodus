# dmgbuild settings for the drag-to-install disk image.
# Defines passed by scripts/package.sh: app (path to Keymodus.app), art (artwork dir).
import os

app = defines["app"]
art = defines["art"]

format = "UDZO"
filesystem = "HFS+"
files = [app]
symlinks = {"Applications": "/Applications"}
icon = os.path.join(app, "Contents/Resources/icon.icns")
badge_icon = None

# Window and icon placement match the arrow in art.swift (640 x 400 pt).
background = os.path.join(art, "dmg-background.png")
window_rect = ((200, 160), (640, 400))
default_view = "icon-view"
show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False
icon_size = 112
text_size = 13
icon_locations = {
    os.path.basename(app): (176, 200),
    "Applications": (464, 200),
}
