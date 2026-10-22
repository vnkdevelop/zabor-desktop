import { useTranslation } from 'react-i18next'
import { useAppStore } from '../../store/useAppStore'
import { webrtcService } from '../../services/webrtc'
import { AvatarImg } from '../Shared/AvatarImg'

export const RemoteControlPill = () => {
  const { t } = useTranslation()
  const currentUser = useAppStore((state) => state.currentUser)
  const isStreamRemoteControlAllowed = useAppStore((state) => state.isStreamRemoteControlAllowed)
  const activeRemoteControllerId = useAppStore((state) => state.activeRemoteControllerId)
  const channelUsers = useAppStore((state) => state.channelUsers)

  if (!currentUser?.isStreaming || !isStreamRemoteControlAllowed) {
    return null
  }

  const isViewerControlling = Boolean(
    activeRemoteControllerId && activeRemoteControllerId !== currentUser.id
  )

  const controllerUser = isViewerControlling
    ? channelUsers.find((u) => u.id === activeRemoteControllerId) || {
        displayName: 'viewer',
        avatarBase64: '',
        avatarColor: '#FF007F'
      }
    : currentUser

  const handlePanicRevoke = () => {
    webrtcService.revokeRemoteControl()
  }

  return (
    <div
      onClick={handlePanicRevoke}
      title={t('stream.revokeRemoteControlHint')}
      className={`fixed top-3 left-1/2 -translate-x-1/2 z-[99999] pointer-events-auto select-none flex items-center gap-2 px-3 py-1.5 rounded-full cursor-pointer transition-all duration-200 hover:scale-105 active:scale-95 ${
        isViewerControlling
          ? 'bg-primary/20 backdrop-blur-xl border border-primary border-t-primary/80 ring-1 ring-primary'
          : 'bg-[#161618]/90 backdrop-blur-xl border border-white/[0.08] border-t-white/[0.16]'
      }`}
    >
      <div className="shrink-0 w-5 h-5 flex items-center justify-center">
        <AvatarImg
          src={controllerUser.avatarBase64}
          size={20}
          bgColor={controllerUser.avatarColor}
          animate={false}
        />
      </div>
      <span className="text-white font-bold text-xs truncate max-w-[120px] lowercase tracking-wide">
        {controllerUser.displayName}
      </span>
    </div>
  )
}
