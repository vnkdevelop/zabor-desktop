import { create } from 'zustand';

interface SpeakingState {
  speaking: Record<string, boolean>;
  setSpeaking: (userId: string, isSpeaking: boolean) => void;
}

export const useSpeakingStore = create<SpeakingState>((set) => ({
  speaking: {},
  setSpeaking: (userId, isSpeaking) => set((state) => {
    if (state.speaking[userId] === isSpeaking) return state;
    return {
      speaking: {
        ...state.speaking,
        [userId]: isSpeaking
      }
    };
  })
}));
