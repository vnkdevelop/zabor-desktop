import { webrtc } from './webrtc';
import { useAppStore } from '../store/useAppStore';

class VoiceMediaManager {
  private static instance: VoiceMediaManager;

  private constructor() { }

  public static getInstance(): VoiceMediaManager {
    if (!VoiceMediaManager.instance) {
      VoiceMediaManager.instance = new VoiceMediaManager();
    }
    return VoiceMediaManager.instance;
  }

  public async startLocalStream(deviceId?: string, useNS?: boolean): Promise<boolean> {
    try {
      const store = useAppStore.getState();
      const actualUseNS = useNS !== undefined ? useNS : store.noiseSuppression;

      const result = await webrtc.startLocalStream(deviceId, actualUseNS);
      return result;
    } catch (error: any) {
      console.error('[VoiceMediaManager] Failed to start local stream:', error);
      this.handleMicrophoneError(error);
      return false;
    }
  }

  public async updateSettings(deviceId: string, useNS: boolean): Promise<void> {
    try {
      useAppStore.getState().setNoiseSuppression(useNS);
      await webrtc.updateSettings(deviceId, useNS);
    } catch (error: any) {
      console.error('[VoiceMediaManager] Failed to update settings:', error);
      this.handleMicrophoneError(error);
    }
  }

  private handleMicrophoneError(error: any) {
    const store = useAppStore.getState();
    const message = error?.message || 'Неизвестная ошибка микрофона';

    if (message.includes('MIC_ACCESS_FAILED') || message.includes('NotAllowedError') || message.includes('PermissionDeniedError')) {
      store.setSystemToast('Нет доступа к микрофону. Проверьте разрешения в ОС.');
    } else if (message.includes('NotReadableError') || message.includes('TrackStartError')) {
      store.setSystemToast('Микрофон занят другим приложением.');
    } else if (message.includes('NotFoundError') || message.includes('DevicesNotFoundError')) {
      store.setSystemToast('Микрофон не найден. Подключите устройство и попробуйте снова.');
    } else {
      store.setSystemToast(`Ошибка аудио: ${message}`);
    }
  }
}

export const voiceMediaManager = VoiceMediaManager.getInstance();
