import SwiftUI

public struct UsageCardView: View {
    public let window: QuotaWindow

    public init(window: QuotaWindow) {
        self.window = window
    }

    private var statusColor: Color {
        switch window.statusLevel {
        case .normal:
            return Color.green
        case .warning:
            return Color.orange
        case .critical:
            return Color.red
        }
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            // Title & Remaining Percentage Row
            HStack(alignment: .firstTextBaseline) {
                Text(window.name)
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundColor(.primary)

                Spacer()

                HStack(spacing: 4) {
                    Text("剩余")
                        .font(.system(size: 11))
                        .foregroundColor(.secondary)
                    Text("\(window.remainingPercent)%")
                        .font(.system(size: 15, weight: .bold, design: .rounded))
                        .foregroundColor(statusColor)
                }
            }

            // Custom Progress Bar
            GeometryReader { geometry in
                ZStack(alignment: .leading) {
                    RoundedRectangle(cornerRadius: 4)
                        .fill(Color.primary.opacity(0.08))
                        .frame(height: 7)

                    RoundedRectangle(cornerRadius: 4)
                        .fill(statusColor)
                        .frame(
                            width: max(0, min(geometry.size.width, geometry.size.width * CGFloat(window.remainingPercent) / 100.0)),
                            height: 7
                        )
                        .animation(.easeInOut(duration: 0.3), value: window.remainingPercent)
                }
            }
            .frame(height: 7)

            // Reset time and countdown row
            HStack(alignment: .center) {
                HStack(spacing: 4) {
                    Image(systemName: "clock")
                        .font(.system(size: 10))
                        .foregroundColor(.secondary)
                    Text("重置时间：\(window.formattedResetTime())")
                        .font(.system(size: 11))
                        .foregroundColor(.secondary)
                }

                Spacer()

                Text(window.countdownString())
                    .font(.system(size: 11, weight: .medium))
                    .foregroundColor(window.statusLevel == .critical ? .red : .secondary)
            }
        }
        .padding(12)
        .background(
            RoundedRectangle(cornerRadius: 8)
                .fill(Color(NSColor.controlBackgroundColor))
        )
        .overlay(
            RoundedRectangle(cornerRadius: 8)
                .stroke(Color.primary.opacity(0.06), lineWidth: 1)
        )
    }
}
