import AppKit
import WebKit

// usage: render <html> <png> <width> <height>
let args = CommandLine.arguments
guard args.count >= 5 else {
    FileHandle.standardError.write("usage: render <html> <png> <w> <h>\n".data(using: .utf8)!)
    exit(2)
}
let htmlPath = args[1]
let outPath = args[2]
let vw = CGFloat(Double(args[3]) ?? 1440)
let vh = CGFloat(Double(args[4]) ?? 900)

let app = NSApplication.shared
app.setActivationPolicy(.accessory)

final class Renderer: NSObject, WKNavigationDelegate {
    let outPath: String
    let vw: CGFloat
    let vh: CGFloat
    var window: NSWindow?
    var webView: WKWebView?

    init(outPath: String, vw: CGFloat, vh: CGFloat) {
        self.outPath = outPath
        self.vw = vw
        self.vh = vh
    }

    func start(htmlPath: String) {
        let cfg = WKWebViewConfiguration()
        let wv = WKWebView(frame: NSRect(x: 0, y: 0, width: vw, height: vh), configuration: cfg)
        wv.navigationDelegate = self
        // Host offscreen so WebKit lays out and paints.
        let win = NSWindow(
            contentRect: NSRect(x: -30000, y: -30000, width: vw, height: vh),
            styleMask: [.borderless],
            backing: .buffered,
            defer: false
        )
        win.contentView = wv
        win.alphaValue = 0
        win.orderFrontRegardless()
        self.window = win
        self.webView = wv

        let url = URL(fileURLWithPath: htmlPath)
        let dir = url.deletingLastPathComponent()
        wv.loadFileURL(url, allowingReadAccessTo: dir)
    }

    func fail(_ msg: String) -> Never {
        FileHandle.standardError.write("render error: \(msg)\n".data(using: .utf8)!)
        exit(1)
    }

    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
        fail(error.localizedDescription)
    }
    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
        fail(error.localizedDescription)
    }

    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.7) {
            let snap = WKSnapshotConfiguration()
            snap.rect = CGRect(x: 0, y: 0, width: self.vw, height: self.vh)
            webView.takeSnapshot(with: snap) { image, error in
                guard let image = image,
                      let tiff = image.tiffRepresentation,
                      let rep = NSBitmapImageRep(data: tiff),
                      let png = rep.representation(using: .png, properties: [:]) else {
                    self.fail("snapshot failed: \(String(describing: error))")
                }
                do {
                    try png.write(to: URL(fileURLWithPath: self.outPath))
                } catch {
                    self.fail("write failed: \(error.localizedDescription)")
                }
                print("wrote \(self.outPath) \(rep.pixelsWide)x\(rep.pixelsHigh)")
                exit(0)
            }
        }
    }
}

let r = Renderer(outPath: outPath, vw: vw, vh: vh)
r.start(htmlPath: htmlPath)

// safety timeout
DispatchQueue.main.asyncAfter(deadline: .now() + 20) {
    FileHandle.standardError.write("render error: timeout\n".data(using: .utf8)!)
    exit(1)
}
app.run()
