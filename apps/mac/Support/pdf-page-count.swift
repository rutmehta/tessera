// Prints the page count and first page size of a PDF (acceptance aid for File ▸ Print… → PDF).
// Usage: swift apps/mac/Support/pdf-page-count.swift file.pdf
import Foundation
import PDFKit

guard CommandLine.arguments.count == 2 else {
    FileHandle.standardError.write(Data("usage: pdf-page-count.swift file.pdf\n".utf8))
    exit(2)
}
guard let doc = PDFDocument(url: URL(fileURLWithPath: CommandLine.arguments[1])) else {
    FileHandle.standardError.write(Data("not a readable PDF\n".utf8))
    exit(1)
}
let box = doc.page(at: 0)?.bounds(for: .mediaBox) ?? .zero
print(String(format: "%d page%@, %.1f × %.1f in", doc.pageCount, doc.pageCount == 1 ? "" : "s",
             box.width / 72, box.height / 72))
