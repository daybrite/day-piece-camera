// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0

// The day-piece-camera crate's OWN iOS backend — a Swift shim over AVFoundation. It is staged
// into the generated `DayPieces` SwiftPM package (docs/extending.md) and exposes a flat C ABI
// (`@_cdecl`) that lib-uikit.rs calls; reports go the other way through the C function pointer
// Rust hands over at creation, always on the main queue. It is the iOS twin of
// platform/android/java/DayCamera.java.

import AVFoundation
import UIKit

/// The report callback: `(node, code, text)`. Codes are src/lib.rs `report::*`.
public typealias DayCameraReport = @convention(c) (UInt64, Double, UnsafePointer<CChar>?) -> Void

private let STOPPED = 0.0
private let STARTING = 1.0
private let RUNNING = 2.0
private let UNAVAILABLE = 3.0
private let DENIED = 4.0
private let ERROR = 5.0
private let PHOTO = 6.0

private let CMD_START: Int32 = 0
private let CMD_STOP: Int32 = 1
private let CMD_CAPTURE: Int32 = 2
private let CMD_FACING: Int32 = 3

/// A UIView whose backing layer IS the preview layer, so the picture follows whatever frame
/// Day sets without a layout pass of its own.
final class DayCameraView: UIView {
    override class var layerClass: AnyClass { AVCaptureVideoPreviewLayer.self }

    var previewLayer: AVCaptureVideoPreviewLayer {
        // The layer class is fixed above, so this cast cannot fail.
        layer as! AVCaptureVideoPreviewLayer
    }

    let node: UInt64
    let report: DayCameraReport
    let session = AVCaptureSession()
    /// Session configuration and start/stop run here, never on the main thread: `startRunning`
    /// blocks for as long as the hardware takes.
    let queue = DispatchQueue(label: "dev.daybrite.day.piece.camera")
    let photoOutput = AVCapturePhotoOutput()
    var facing: AVCaptureDevice.Position
    var input: AVCaptureDeviceInput?
    /// Capture delegates stay alive until their photo lands; AVFoundation holds them weakly.
    var inFlight: [DayCameraCapture] = []
    var rotation: AnyObject?

    init(node: UInt64, facing: AVCaptureDevice.Position, report: @escaping DayCameraReport) {
        self.node = node
        self.facing = facing
        self.report = report
        super.init(frame: .zero)
        backgroundColor = .black
        previewLayer.videoGravity = .resizeAspectFill
        previewLayer.session = session
    }

    required init?(coder: NSCoder) { nil }

    func send(_ code: Double, _ text: String = "") {
        let node = self.node
        let report = self.report
        DispatchQueue.main.async {
            text.withCString { report(node, code, $0) }
        }
    }

    /// Configure (or reconfigure, on a camera switch) and run the session.
    func start() {
        send(STARTING)
        queue.async { [self] in
            guard AVCaptureDevice.authorizationStatus(for: .video) == .authorized else {
                send(DENIED)
                return
            }
            guard let device = AVCaptureDevice.default(.builtInWideAngleCamera, for: .video, position: facing)
                ?? AVCaptureDevice.default(for: .video)
            else {
                send(UNAVAILABLE)
                return
            }
            do {
                session.beginConfiguration()
                if session.canSetSessionPreset(.photo) {
                    session.sessionPreset = .photo
                }
                if let old = input {
                    session.removeInput(old)
                }
                let next = try AVCaptureDeviceInput(device: device)
                guard session.canAddInput(next) else {
                    session.commitConfiguration()
                    send(ERROR, "the camera input could not be added")
                    return
                }
                session.addInput(next)
                input = next
                if !session.outputs.contains(photoOutput) {
                    guard session.canAddOutput(photoOutput) else {
                        session.commitConfiguration()
                        send(ERROR, "the photo output could not be added")
                        return
                    }
                    session.addOutput(photoOutput)
                }
                photoOutput.maxPhotoQualityPrioritization = .balanced
                session.commitConfiguration()
            } catch {
                session.commitConfiguration()
                send(ERROR, error.localizedDescription)
                return
            }
            keepUpright(device)
            if !session.isRunning {
                session.startRunning()
            }
            send(session.isRunning ? RUNNING : ERROR, session.isRunning ? "" : "the session did not start")
        }
    }

    func stop() {
        queue.async { [self] in
            if session.isRunning {
                session.stopRunning()
            }
            send(STOPPED)
        }
    }

    func capture() {
        queue.async { [self] in
            guard session.isRunning else {
                send(ERROR, "the camera is not running")
                return
            }
            let settings = photoOutput.availablePhotoCodecTypes.contains(.jpeg)
                ? AVCapturePhotoSettings(format: [AVVideoCodecKey: AVVideoCodecType.jpeg])
                : AVCapturePhotoSettings()
            if let connection = photoOutput.connection(with: .video) {
                applyRotation(to: connection)
            }
            let delegate = DayCameraCapture(view: self)
            inFlight.append(delegate)
            photoOutput.capturePhoto(with: settings, delegate: delegate)
        }
    }

    func switchFacing(_ position: AVCaptureDevice.Position) {
        guard position != facing else { return }
        facing = position
        if session.isRunning || input != nil {
            start()
        }
    }

    /// Keep the preview and the capture upright. iOS 17's rotation coordinator does it for both;
    /// below it the preview follows the interface orientation at start.
    func keepUpright(_ device: AVCaptureDevice) {
        if #available(iOS 17.0, *) {
            let coordinator = AVCaptureDevice.RotationCoordinator(device: device, previewLayer: previewLayer)
            rotation = coordinator
            DispatchQueue.main.async { [self] in
                if let connection = previewLayer.connection,
                    connection.isVideoRotationAngleSupported(coordinator.videoRotationAngleForHorizonLevelPreview)
                {
                    connection.videoRotationAngle = coordinator.videoRotationAngleForHorizonLevelPreview
                }
            }
        } else {
            DispatchQueue.main.async { [self] in
                if let connection = previewLayer.connection, connection.isVideoOrientationSupported {
                    connection.videoOrientation = .portrait
                }
            }
        }
    }

    func applyRotation(to connection: AVCaptureConnection) {
        if #available(iOS 17.0, *) {
            if let coordinator = rotation as? AVCaptureDevice.RotationCoordinator,
                connection.isVideoRotationAngleSupported(coordinator.videoRotationAngleForHorizonLevelCapture)
            {
                connection.videoRotationAngle = coordinator.videoRotationAngleForHorizonLevelCapture
            }
        } else if connection.isVideoOrientationSupported {
            connection.videoOrientation = .portrait
        }
    }

    func finished(_ delegate: DayCameraCapture) {
        queue.async { [self] in
            inFlight.removeAll { $0 === delegate }
        }
    }

    /// Release: stop the session and drop the preview, whatever thread asks.
    func tearDown() {
        queue.async { [self] in
            if session.isRunning {
                session.stopRunning()
            }
        }
    }
}

/// One capture: writes the JPEG into the app's cache directory and reports its path and size.
final class DayCameraCapture: NSObject, AVCapturePhotoCaptureDelegate {
    weak var view: DayCameraView?

    init(view: DayCameraView) {
        self.view = view
    }

    func photoOutput(_ output: AVCapturePhotoOutput, didFinishProcessingPhoto photo: AVCapturePhoto, error: Error?) {
        defer { view?.finished(self) }
        guard let view = view else { return }
        if let error = error {
            view.send(ERROR, error.localizedDescription)
            return
        }
        guard let raw = photo.fileDataRepresentation() else {
            view.send(ERROR, "the photo has no file representation")
            return
        }
        // The sensor's pixels carry an EXIF rotation. A photo is handed over UPRIGHT, so a
        // viewer that ignores EXIF shows it right too: redraw when the orientation is not `.up`.
        var data = raw
        var width = 0
        var height = 0
        if let image = UIImage(data: raw) {
            if image.imageOrientation != .up {
                let renderer = UIGraphicsImageRenderer(size: image.size)
                let redrawn = renderer.image { _ in image.draw(in: CGRect(origin: .zero, size: image.size)) }
                if let jpeg = redrawn.jpegData(compressionQuality: 0.92) {
                    data = jpeg
                }
            }
            width = Int(image.size.width * image.scale)
            height = Int(image.size.height * image.scale)
        }
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("day-camera", isDirectory: true)
        let file = dir.appendingPathComponent(UUID().uuidString + ".jpg")
        do {
            try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
            try data.write(to: file, options: .atomic)
        } catch {
            view.send(ERROR, error.localizedDescription)
            return
        }
        view.send(PHOTO, "\(file.path)\u{1F}\(width)\u{1F}\(height)")
    }
}

/// Create the preview view and, when `active`, start the session. Returns a +1-retained pointer
/// — the Rust caller takes ownership (wraps it as Retained<UIView>).
@_cdecl("day_camera_new")
public func day_camera_new(
    _ node: UInt64,
    _ facing: Int32,
    _ active: Bool,
    _ report: DayCameraReport
) -> UnsafeMutableRawPointer {
    let view = DayCameraView(node: node, facing: facing == 1 ? .front : .back, report: report)
    if active {
        view.start()
    }
    return Unmanaged.passRetained(view).toOpaque()
}

/// A command from a `CameraPatch`: start, stop, capture, or switch cameras (`arg` = facing).
@_cdecl("day_camera_command")
public func day_camera_command(_ viewPtr: UnsafeMutableRawPointer, _ cmd: Int32, _ arg: Int32) {
    let view = Unmanaged<DayCameraView>.fromOpaque(viewPtr).takeUnretainedValue()
    switch cmd {
    case CMD_START: view.start()
    case CMD_STOP: view.stop()
    case CMD_CAPTURE: view.capture()
    case CMD_FACING: view.switchFacing(arg == 1 ? .front : .back)
    default: break
    }
}

/// The renderer's release: stop the session. Rust still owns the +1 and drops it after this.
@_cdecl("day_camera_release")
public func day_camera_release(_ viewPtr: UnsafeMutableRawPointer) {
    let view = Unmanaged<DayCameraView>.fromOpaque(viewPtr).takeUnretainedValue()
    view.tearDown()
}
