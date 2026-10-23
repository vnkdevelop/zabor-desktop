import { useTranslation } from 'react-i18next'
import { useAppStore } from '../../store/useAppStore'
import { webrtcService } from '../../services/webrtc'
import { AvatarImg } from '../Shared/AvatarImg'

export const RemoteControlPill = () => {
  const { t } = useTranslation()
  const currentUser = useAppStore((state) => state.currentUser)
  const isStreamRemoteControlAllowed = useAppStore((state) => state.isStreamRemoteControlAllowed)
  const activeRemoteControllerId = useAppStore((state) => state.activeRemoteControllerId)
  const voiceUsers = useAppStore((state) => state.voiceUsers)
  const friends = useAppStore((state) => state.friends)
  const currentCallUser = useAppStore((state) => state.currentCallUser)
  const channelUsersMap = useAppStore((state) => state.channelUsersMap)
  const currentChannelId = useAppStore((state) => state.currentChannelId)

  const isHostAllowed = Boolean(currentUser?.isStreaming && isStreamRemoteControlAllowed)
  const isViewerControlling = Boolean(
    activeRemoteControllerId && activeRemoteControllerId !== currentUser?.id
  )

  if (!isHostAllowed && !isViewerControlling) {
    return null
  }

  const channelMembers = currentChannelId ? channelUsersMap[currentChannelId] || [] : []
  const allUsers = [
    ...(voiceUsers || []),
    ...(friends || []),
    ...channelMembers,
    ...(currentCallUser ? [currentCallUser] : [])
  ]

  const controllerUser = isViewerControlling
    ? allUsers.find((u) => u?.id === activeRemoteControllerId) || {
        displayName: 'viewer',
        avatarBase64: '',
        avatarColor: '#FF007F'
      }
    : currentUser || {
        displayName: 'host',
        avatarBase64: '',
        avatarColor: '#FF007F'
      }

  const handlePanicRevoke = () => {
    webrtcService.revokeRemoteControl()
  }

  return (
    <div
      onClick={handlePanicRevoke}
      title={t('stream.revokeRemoteControlHint')}
      style={{ WebkitAppRegion: 'no-drag' as any }}
      className={`title-no-drag fixed top-2 left-1/2 -translate-x-1/2 z-[999999] pointer-events-auto select-none flex items-center gap-2 px-3 py-1.5 rounded-full cursor-pointer transition-all duration-200 hover:scale-105 active:scale-95 ${
        isViewerControlling
          ? 'bg-[#161618]/95 backdrop-blur-xl border border-primary border-t-primary/90 shadow-[0_0_16px_rgba(255,0,127,0.35)]'
          : 'bg-[#161618]/90 backdrop-blur-xl border border-white/[0.08] border-t-white/[0.16]'
      }`}
    >
      {isViewerControlling ? (
        <>
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
          <span className="text-primaryText font-extrabold text-[10px] tracking-wide ml-0.5 lowercase">
            ✕ {t('stream.stopHint', 'отозвать')}
          </span>
        </>
      ) : (
        <>
          <div className="w-2 h-2 rounded-full bg-primary animate-pulse" />
          <span className="text-white/80 font-bold text-[11px] truncate lowercase tracking-wide">
            {t('stream.remoteControlAllowedHint', 'удаленное управление разрешено')}
          </span>
        </>
      )}
    </div>
  )
}
