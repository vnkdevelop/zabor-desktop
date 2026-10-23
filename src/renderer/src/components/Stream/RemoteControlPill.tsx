import { useAppStore } from '../../store/useAppStore'

export const RemoteControlPill = () => {
  const currentUser = useAppStore((state) => state.currentUser)
  const activeRemoteControllerId = useAppStore((state) => state.activeRemoteControllerId)
  const voiceUsers = useAppStore((state) => state.voiceUsers)
  const friends = useAppStore((state) => state.friends)
  const currentCallUser = useAppStore((state) => state.currentCallUser)
  const channelUsersMap = useAppStore((state) => state.channelUsersMap)
  const currentChannelId = useAppStore((state) => state.currentChannelId)

  const isViewerControlling = Boolean(
    activeRemoteControllerId && activeRemoteControllerId !== currentUser?.id
  )

  if (!isViewerControlling) {
    return null
  }

  const channelMembers = currentChannelId ? channelUsersMap[currentChannelId] || [] : []
  const allUsers = [
    ...(voiceUsers || []),
    ...(friends || []),
    ...channelMembers,
    ...(currentCallUser ? [currentCallUser] : [])
  ]

  const controllerUser = allUsers.find((u) => u?.id === activeRemoteControllerId) || {
    displayName: 'viewer'
  }

  return (
    <div
      style={{ WebkitAppRegion: 'no-drag' as any }}
      className="title-no-drag fixed top-2 left-1/2 -translate-x-1/2 z-[999999] pointer-events-none select-none flex items-center gap-2 px-3 py-1 rounded-full bg-[#161618]/90 backdrop-blur-xl border border-white/[0.08] border-t-white/[0.14]"
    >
      <div className="w-1.5 h-1.5 rounded-full bg-white shrink-0" />
      <span className="text-white/90 font-medium text-xs truncate max-w-[130px] lowercase tracking-wide">
        {controllerUser.displayName}
      </span>
    </div>
  )
}
