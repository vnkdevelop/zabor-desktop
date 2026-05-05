import { contextBridge, ipcRenderer } from 'electron'

const windowControls = {
  minimize: () => ipcRenderer.send('window-minimize'),
  maximize: () => ipcRenderer.send('window-maximize'),
  close: () => ipcRenderer.send('window-close'),
  quit: () => ipcRenderer.send('app-quit'),
  wipeAppData: () => ipcRenderer.invoke('wipe-app-data'),
  getUserDataPath: () => ipcRenderer.invoke('get-userdata-path'),
  getAutoLaunch: (): Promise<boolean> => ipcRenderer.invoke('get-auto-launch'),
  setAutoLaunch: (enabled: boolean): Promise<boolean> => ipcRenderer.invoke('set-auto-launch', enabled),
  getMinimizeToTray: (): Promise<boolean> => ipcRenderer.invoke('get-minimize-to-tray'),
  setMinimizeToTray: (enabled: boolean): Promise<boolean> => ipcRenderer.invoke('set-minimize-to-tray', enabled),
  saveSession: (data: string): Promise<boolean> => ipcRenderer.invoke('save-session', data),
  loadSession: (): Promise<string | null> => ipcRenderer.invoke('load-session'),
  clearSession: (): Promise<boolean> => ipcRenderer.invoke('clear-session'),
  onBeforeQuit: (callback: () => void) => {
    ipcRenderer.on('before-quit', callback)
    return () => { ipcRenderer.removeAllListeners('before-quit') }
  },
  onMaximizeChange: (callback: (isMaximized: boolean) => void) => {
    ipcRenderer.on('window-maximized', () => callback(true))
    ipcRenderer.on('window-unmaximized', () => callback(false))
    return () => {
      ipcRenderer.removeAllListeners('window-maximized')
      ipcRenderer.removeAllListeners('window-unmaximized')
    }
  }
}

contextBridge.exposeInMainWorld('windowControls', windowControls)