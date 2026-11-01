import AppKit
import CoreAudio

enum AgentAlertPlaybackResult: Equatable {
    /// AppKit accepted the local playback request. This does not prove a person heard it.
    case requested
    case suppressed
    case unavailable(String)
}

/// The only sound path for provider lifecycle alerts and previews.
@MainActor
final class AgentAlertAudioPlayer {
    let preferences: AgentAlertPreferences

    init(preferences: AgentAlertPreferences) {
        self.preferences = preferences
    }

    func playEvent(_ kind: AgentAlertKind) -> AgentAlertPlaybackResult {
        guard preferences.playSounds else { return .suppressed }
        return play(preferences.sound(for: kind))
    }

    /// Preview intentionally ignores the event master switch and does not create a notification.
    func preview(_ kind: AgentAlertKind) -> AgentAlertPlaybackResult {
        play(preferences.sound(for: kind))
    }

    func availability(for kind: AgentAlertKind) -> String? {
        let sound = preferences.sound(for: kind)
        guard NSSound(named: NSSound.Name(sound.rawValue)) != nil else {
            return "The selected macOS sound is unavailable."
        }
        if Self.defaultOutputIsMuted == true { return "The macOS output device is muted." }
        return nil
    }

    private func play(_ sound: AgentAlertSound) -> AgentAlertPlaybackResult {
        if Self.defaultOutputIsMuted == true { return .unavailable("The macOS output device is muted") }
        guard let player = NSSound(named: NSSound.Name(sound.rawValue)) else {
            return .unavailable("The selected macOS sound is unavailable")
        }
        player.volume = 1
        guard player.play() else { return .unavailable("macOS did not start the selected sound") }
        return .requested
    }

    /// Returns nil when the output device does not expose a mute control.
    private static var defaultOutputIsMuted: Bool? {
        var deviceAddress = AudioObjectPropertyAddress(
            mSelector: kAudioHardwarePropertyDefaultOutputDevice,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var device = AudioDeviceID(kAudioObjectUnknown)
        var size = UInt32(MemoryLayout<AudioDeviceID>.size)
        guard AudioObjectGetPropertyData(
            AudioObjectID(kAudioObjectSystemObject), &deviceAddress, 0, nil, &size, &device
        ) == noErr, device != AudioDeviceID(kAudioObjectUnknown) else { return nil }

        var muteAddress = AudioObjectPropertyAddress(
            mSelector: kAudioDevicePropertyMute,
            mScope: kAudioDevicePropertyScopeOutput,
            mElement: kAudioObjectPropertyElementMain
        )
        var statusKnown = false
        if AudioObjectHasProperty(device, &muteAddress) {
            var muted: UInt32 = 0
            size = UInt32(MemoryLayout<UInt32>.size)
            if AudioObjectGetPropertyData(device, &muteAddress, 0, nil, &size, &muted) == noErr {
                statusKnown = true
                if muted != 0 { return true }
            }
        }

        var volumeAddress = AudioObjectPropertyAddress(
            mSelector: kAudioDevicePropertyVolumeScalar,
            mScope: kAudioDevicePropertyScopeOutput,
            mElement: kAudioObjectPropertyElementMain
        )
        if AudioObjectHasProperty(device, &volumeAddress) {
            var volume: Float32 = 1
            size = UInt32(MemoryLayout<Float32>.size)
            if AudioObjectGetPropertyData(device, &volumeAddress, 0, nil, &size, &volume) == noErr {
                statusKnown = true
                if volume.isFinite && volume <= 0 { return true }
            }
        }
        return statusKnown ? false : nil
    }
}
