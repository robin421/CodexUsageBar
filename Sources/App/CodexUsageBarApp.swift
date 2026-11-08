import SwiftUI
import AppKit

@main
struct CodexUsageBarApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) var appDelegate
    @StateObject private var viewModel = UsageViewModel.shared

    private var iconColor: Color {
        switch viewModel.menuBarStatusLevel {
        case .normal:
            return .primary
        case .warning:
            return .orange
        case .critical:
            return .red
        }
    }

    var body: some Scene {
        MenuBarExtra {
            UsagePopoverView(viewModel: viewModel, onOpenSettings: {
                SettingsWindowController.shared.showSettings(viewModel: viewModel)
            })
        } label: {
            HStack(spacing: 4) {
                Image("MenuBarIcon")
                    .renderingMode(.template)
                    .foregroundColor(iconColor)

                if viewModel.menuBarStyle != .iconOnly && !viewModel.menuBarTitle.isEmpty {
                    Text(viewModel.menuBarTitle)
                        .font(.system(size: 12, weight: .medium))
                }
            }
        }
        .menuBarExtraStyle(.window)

        Settings {
            SettingsView(viewModel: viewModel)
        }
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        // Request notification authorization on first run
        NotificationManager.shared.requestAuthorization()
    }

    func applicationWillTerminate(_ notification: Notification) {
        // Clean up subprocess
    }
}
