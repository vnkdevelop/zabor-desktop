/// <reference types="vite/client" />

export {};

declare global {
  interface Window {
    electron: {
      process: {
        versions: Record<string, string>;
      };
    };
    windowControls: {
      minimize: () => void;
      maximize: () => void;
      close: () => void;
      quit: () => void;
      wipeAppData: () => Promise<boolean>;
      getUserDataPath: () => Promise<string>;
      getAutoLaunch: () => Promise<boolean>;
      setAutoLaunch: (enabled: boolean) => Promise<boolean>;
      saveSession: (data: string) => Promise<boolean>;
      loadSession: () => Promise<string | null>;
      clearSession: () => Promise<boolean>;
      onMaximizeChange: (callback: (isMaximized: boolean) => void) => () => void;
      onBeforeQuit: (callback: () => void) => () => void;
    };
  }
}

declare module '*.mp3' {
  const src: string;
  export default src;
}