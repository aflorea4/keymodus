// Renders the installer artwork in the app's brutalist palette: concrete /
// carbon, hard black rules, one acid-volt signal, extra-wide headline type.
//   swift scripts/installer/art.swift <app-icon.png> <out-dir>
// Writes dmg-background.png (+ @2x) and pkg-background.png / pkg-background-dark.png (+ @2x).
import AppKit

let args = CommandLine.arguments
guard args.count == 3, let icon = NSImage(contentsOfFile: args[1]) else {
    FileHandle.standardError.write("usage: art.swift <icon.png> <out-dir>\n".data(using: .utf8)!)
    exit(1)
}
let out = URL(fileURLWithPath: args[2])
try? FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)

func rgb(_ hex: UInt32, _ a: CGFloat = 1) -> NSColor {
    NSColor(srgbRed: CGFloat((hex >> 16) & 0xff) / 255, green: CGFloat((hex >> 8) & 0xff) / 255, blue: CGFloat(hex & 0xff) / 255, alpha: a)
}
let concrete = rgb(0xE6E6E0), ink = rgb(0x1A1B16), muted = rgb(0x5A5B52)
let bone = rgb(0xEFEFE6)
let volt = rgb(0xDDF020)

/// SF, extra-wide and black: the closest system match to the app's Archivo Expanded.
func wide(_ size: CGFloat) -> NSFont {
    if #available(macOS 13.0, *) { return NSFont.systemFont(ofSize: size, weight: .black, width: .expanded) }
    return NSFont.systemFont(ofSize: size, weight: .black)
}
func mono(_ size: CGFloat) -> NSFont { NSFont.monospacedSystemFont(ofSize: size, weight: .medium) }

/// Draws in points with a top-left origin, rendered at `scale`.
func render(_ name: String, _ w: CGFloat, _ h: CGFloat, scale: CGFloat, _ draw: (CGContext) -> Void) {
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: Int(w * scale), pixelsHigh: Int(h * scale),
                               bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                               colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    rep.size = NSSize(width: w, height: h)
    let ctx = NSGraphicsContext(bitmapImageRep: rep)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = ctx
    let cg = ctx.cgContext
    cg.translateBy(x: 0, y: h)
    cg.scaleBy(x: 1, y: -1)
    NSGraphicsContext.current = NSGraphicsContext(cgContext: cg, flipped: true)
    draw(cg)
    NSGraphicsContext.restoreGraphicsState()
    try! rep.representation(using: .png, properties: [:])!.write(to: out.appendingPathComponent(name))
}

func text(_ s: String, _ font: NSFont, _ color: NSColor, at p: CGPoint, tracking: CGFloat = 0, alignRight: Bool = false) {
    let attrs: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: color, .kern: tracking]
    let size = (s as NSString).size(withAttributes: attrs)
    (s as NSString).draw(at: CGPoint(x: alignRight ? p.x - size.width : p.x, y: p.y), withAttributes: attrs)
}

func rule(_ cg: CGContext, _ x0: CGFloat, _ y0: CGFloat, _ x1: CGFloat, _ y1: CGFloat, _ color: NSColor, _ w: CGFloat = 1) {
    cg.setStrokeColor(color.cgColor); cg.setLineWidth(w); cg.setLineCap(.butt)
    cg.move(to: CGPoint(x: x0, y: y0)); cg.addLine(to: CGPoint(x: x1, y: y1)); cg.strokePath()
}

func grid(_ cg: CGContext, _ w: CGFloat, _ h: CGFloat, _ color: NSColor, step: CGFloat) {
    var x: CGFloat = 0.5
    while x < w { rule(cg, x, 0, x, h, color, 1); x += step }
    var y: CGFloat = 0.5
    while y < h { rule(cg, 0, y, w, y, color, 1); y += step }
}

/// Four crop-mark corners around a square centred at `c`.
func cropMarks(_ cg: CGContext, center c: CGPoint, half: CGFloat, len: CGFloat, color: NSColor) {
    for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] as [(CGFloat, CGFloat)] {
        let x = c.x + sx * half, y = c.y + sy * half
        rule(cg, x, y, x - sx * len, y, color, 1.5)
        rule(cg, x, y, x, y - sy * len, color, 1.5)
    }
}

// MARK: DMG window, 640 x 400 pt. Finder places the app icon at (176, 200) and
// the Applications link at (464, 200); keep in sync with dmg-settings.py.
// Light ground on purpose: Finder draws icon labels in dark text.
for scale in [1.0, 2.0] as [CGFloat] {
    render(scale == 1 ? "dmg-background.png" : "dmg-background@2x.png", 640, 400, scale: scale) { cg in
        cg.setFillColor(concrete.cgColor)
        cg.fill(CGRect(x: 0, y: 0, width: 640, height: 400))
        grid(cg, 640, 400, ink.withAlphaComponent(0.05), step: 20)

        // Header block
        text("KEYMODUS", wide(30), ink, at: CGPoint(x: 32, y: 24), tracking: -0.3)
        text("INPUT CONTROL / INSTALL", mono(9.5), muted, at: CGPoint(x: 34, y: 66), tracking: 1.1)
        text("V0.1", mono(9.5), muted, at: CGPoint(x: 608, y: 66), tracking: 1.1, alignRight: true)
        rule(cg, 0, 90.5, 640, 90.5, ink, 1.5)

        // Targets
        cropMarks(cg, center: CGPoint(x: 176, y: 200), half: 76, len: 12, color: ink.withAlphaComponent(0.7))
        cropMarks(cg, center: CGPoint(x: 464, y: 200), half: 76, len: 12, color: ink.withAlphaComponent(0.7))
        text("01 / APP", mono(9), muted, at: CGPoint(x: 104, y: 108), tracking: 1)
        text("02 / DESTINATION", mono(9), muted, at: CGPoint(x: 392, y: 108), tracking: 1)

        // Arrow: a volt bar with a hard shadow and a square-cut head.
        let shaft = CGRect(x: 268, y: 194, width: 90, height: 12)
        func arrow(_ dx: CGFloat, _ color: NSColor) {
            cg.setFillColor(color.cgColor)
            cg.fill(shaft.offsetBy(dx: dx, dy: dx))
            cg.move(to: CGPoint(x: 356 + dx, y: 182 + dx))
            cg.addLine(to: CGPoint(x: 378 + dx, y: 200 + dx))
            cg.addLine(to: CGPoint(x: 356 + dx, y: 218 + dx))
            cg.closePath(); cg.fillPath()
        }
        arrow(3, ink)
        arrow(0, volt)

        // Footer
        rule(cg, 0, 352.5, 640, 352.5, ink, 1.5)
        cg.setFillColor(volt.cgColor)
        cg.fill(CGRect(x: 32, y: 370, width: 8, height: 8))
        text("FIRST LAUNCH: ALLOW ACCESSIBILITY WHEN MACOS ASKS", mono(9.5), ink, at: CGPoint(x: 48, y: 367), tracking: 1)
    }
}

// MARK: Installer.app sidebar art (bottom-left aligned, unscaled), light and dark.
for dark in [false, true] {
    for scale in [1.0, 2.0] as [CGFloat] {
        let name = "pkg-background\(dark ? "-dark" : "")\(scale == 2 ? "@2x" : "").png"
        let fg = dark ? bone : ink
        render(name, 220, 260, scale: scale) { cg in
            cropMarks(cg, center: CGPoint(x: 92, y: 150), half: 68, len: 10, color: fg.withAlphaComponent(0.45))
            NSGraphicsContext.current?.imageInterpolation = .high
            icon.draw(in: CGRect(x: 36, y: 94, width: 112, height: 112), from: .zero, operation: .sourceOver,
                      fraction: 1, respectFlipped: true, hints: nil)
            text("KEYMODUS", wide(15), fg.withAlphaComponent(0.85), at: CGPoint(x: 24, y: 228), tracking: -0.2)
        }
    }
}
print("art written to \(out.path)")
