import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

guard CommandLine.arguments.count == 3 else {
    fputs("usage: mask_macos_icon.swift INPUT.png OUTPUT.png\n", stderr)
    exit(2)
}

let input = URL(fileURLWithPath: CommandLine.arguments[1])
let output = URL(fileURLWithPath: CommandLine.arguments[2])
guard
    let source = CGImageSourceCreateWithURL(input as CFURL, nil),
    let image = CGImageSourceCreateImageAtIndex(source, 0, nil)
else {
    fputs("unable to read input PNG\n", stderr)
    exit(3)
}

let width = image.width
let height = image.height
guard width == 1254, height == 1254 else {
    fputs("expected the approved 1254x1254 source image\n", stderr)
    exit(4)
}

let colorSpace = CGColorSpaceCreateDeviceRGB()
guard let context = CGContext(
    data: nil,
    width: width,
    height: height,
    bitsPerComponent: 8,
    bytesPerRow: width * 4,
    space: colorSpace,
    bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
) else {
    fputs("unable to create RGBA context\n", stderr)
    exit(5)
}

context.interpolationQuality = .high
let tileRect = CGRect(x: 70, y: 97, width: 1114, height: 1104)
context.addPath(CGPath(
    roundedRect: tileRect,
    cornerWidth: 227,
    cornerHeight: 227,
    transform: nil
))
context.clip()
context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))

guard
    let masked = context.makeImage(),
    let destination = CGImageDestinationCreateWithURL(
        output as CFURL,
        UTType.png.identifier as CFString,
        1,
        nil
    )
else {
    fputs("unable to prepare output PNG\n", stderr)
    exit(6)
}

CGImageDestinationAddImage(destination, masked, nil)
guard CGImageDestinationFinalize(destination) else {
    fputs("unable to finalize output PNG\n", stderr)
    exit(7)
}
