import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { CornersIn, CornersOut } from '@phosphor-icons/react'
import { animate, spring, utils, prefersReducedMotion } from '../../lib/anime'
import { useTranslation } from 'react-i18next'
import { User, useAppStore } from '../../store/useAppStore'
import { AvatarImg } from '../Shared/AvatarImg'
import { webrtcService } from '../../services/webrtc'
import {
  RC_OP_PING,
  RC_OP_RESET,
  RC_OP_START,
  RC_OP_STOP,
  getWinVkCode,
  isShortcutSafe
} from '../../services/remoteControl'

function getVideoRenderRect(video: HTMLVideoElement) {
  const cw = video.clientWidth
  const ch = video.clientHeight
  const vw = video.videoWidth
  const vh = video.videoHeight
  if (!vw || !vh || !cw || !ch) {
    return { left: 0, top: 0, width: cw, height: ch }
  }
  const videoRatio = vw / vh
  const elemRatio = cw / ch
  let renderW = cw
  let renderH = ch
  let offsetX = 0
  let offsetY = 0
  if (elemRatio > videoRatio) {
    renderW = ch * videoRatio
    offsetX = (cw - renderW) / 2
  } else {
    renderH = cw / videoRatio
    offsetY = (ch - renderH) / 2
  }
  return { left: offsetX, top: offsetY, width: renderW, height: renderH }
}

const StreamLoadingDots = ({ compact, label }: { compact: boolean; label: string }) => (
  <div
    role="status"
    aria-label={label}
    className="absolute inset-0 z-10 flex items-center justify-center bg-black/40 backdrop-blur-[2px] pointer-events-none"
  >
    <div className={`flex ${compact ? 'gap-1.5' : 'gap-2.5'}`}>
      <div className={`${compact ? 'w-2 h-2' : 'w-3 h-3'} bg-primary rounded-full animate-pulse`} />
      <div className={`${compact ? 'w-2 h-2' : 'w-3 h-3'} bg-primary rounded-full animate-pulse`} style={{ animationDelay: '0.15s' }} />
      <div className={`${compact ? 'w-2 h-2' : 'w-3 h-3'} bg-primary rounded-full animate-pulse`} style={{ animationDelay: '0.3s' }} />
    </div>
  </div>
)

const StreamOwnerPill = ({ user, cardWidth, compact }: { user: User; cardWidth: number; compact: boolean }) => (
  <div className="absolute inset-0 z-20 flex items-center justify-center pointer-events-none">
    <div
      className={`bg-[#09090B]/80 backdrop-blur-md border border-[#303035]/50 rounded-full flex items-center whitespace-nowrap ${compact ? 'gap-1.5 px-2 py-0.5' : 'gap-2 px-3 py-1'}`}
      style={{ maxWidth: `${Math.max(60, cardWidth - 20)}px` }}
    >
      <div className="shrink-0" style={{ width: compact ? 16 : 20, height: compact ? 16 : 20 }}>
        <AvatarImg src={user.avatarBase64} size={compact ? 16 : 20} bgColor={user.avatarColor} animate={false} />
      </div>
      <span className={`text-white font-bold truncate ${compact ? 'text-[11px]' : 'text-sm'}`}>
        {user.displayName}
      </span>
    </div>
  </div>
)

interface StreamCardProps {
  user: User
  stream: MediaStream | null
  cardSize: { w: number; h: number }
  isFocused: boolean
  isFullscreen?: boolean
  showOverlays?: boolean
  onClick: () => void
  onContextMenu: (e: React.MouseEvent) => void
  onToggleFullscreen?: () => void
  onRatioChange?: (ratio: number) => void
}

export const StreamCard = ({
  user,
  stream,
  cardSize,
  isFocused,
  isFullscreen,
  showOverlays = true,
  onClick,
  onContextMenu,
  onToggleFullscreen,
  onRatioChange
}: StreamCardProps) => {
  const { t } = useTranslation()
  const rootRef = useRef<HTMLDivElement>(null)
  const videoRef = useRef<HTMLVideoElement | null>(null)
  const captureVideoRef = useRef<HTMLVideoElement | null>(null)
  const [snapshot, setSnapshot] = useState<string | null>(null)
  const [isCapturing, setIsCapturing] = useState(false)
  const [hasFirstFrame, setHasFirstFrame] = useState(false)
  const retryTimerRef = useRef<NodeJS.Timeout | null>(null)
  const captureTimeoutRef = useRef<NodeJS.Timeout | null>(null)

  const currentUserId = useAppStore((state) => state.currentUser?.id)
  const isLocal = user.id === currentUserId

  const mode = isFullscreen ? 'fullscreen' : (isFocused ? 'focused' : 'normal')
  const compactPill = cardSize.w <= 200

  const isRemoteControlling = useAppStore((state) => state.isRemoteControlling)
  const isHostActive = useAppStore((state) => state.isHostActive)
  const isRemoteControlRevoked = useAppStore((state) => state.isRemoteControlRevoked)
  const setRemoteControlState = useAppStore((state) => state.setRemoteControlState)

  const pendingMove = useRef<{ x: number; y: number } | null>(null)
  const virtualPos = useRef<{ x: number; y: number }>({ x: 0.5, y: 0.5 })
  const lastMoveTime = useRef(0)
  const moveThrottleTimer = useRef<NodeJS.Timeout | null>(null)
  const [isCursorHidden, setIsCursorHidden] = useState(false)

  useEffect(() => {
    if (!isRemoteControlling) {
      setIsCursorHidden(false)
      setRemoteControlState({ isCursorHidden: false })
      try {
        window.windowControls.remoteControlUnclipCursor()
      } catch { }
      if (document.pointerLockElement) {
        try { document.exitPointerLock() } catch { }
      }
    }
  }, [isRemoteControlling, setRemoteControlState])

  useEffect(() => {
    const handleLockChange = () => {
      const isLocked = Boolean(document.pointerLockElement && document.pointerLockElement === videoRef.current)
      if (!isLocked && isCursorHidden) {
        setIsCursorHidden(false)
        const msg = t('stream.cursorShown', 'курсор показан')
        useAppStore.getState().setSystemToast(msg)
        setTimeout(() => {
          if (useAppStore.getState().systemToast === msg) {
            useAppStore.getState().setSystemToast(null)
          }
        }, 2000)
      }
    }
    document.addEventListener('pointerlockchange', handleLockChange)
    return () => document.removeEventListener('pointerlockchange', handleLockChange)
  }, [isCursorHidden, t])

  useEffect(() => {
    if (!isFullscreen || isLocal) {
      if (isRemoteControlling) {
        webrtcService.sendViewerSignal(user.id, RC_OP_STOP)
        webrtcService.sendViewerSignal(user.id, RC_OP_RESET)
        setRemoteControlState({ isControlling: false })
        if (document.pointerLockElement) {
          try { document.exitPointerLock() } catch { }
        }
      }
      return
    }

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.altKey && (e.code === 'KeyK' || e.key.toLowerCase() === 'k')) {
        e.preventDefault()
        e.stopPropagation()
        e.stopImmediatePropagation?.()
        if (isRemoteControlRevoked) return
        const next = !isRemoteControlling
        if (next) {
          webrtcService.ensureRemoteControlChannelForStreamer(user.id)
          setRemoteControlState({ isControlling: true, isHostActive: false })
          webrtcService.sendViewerSignal(user.id, RC_OP_START)
        } else {
          webrtcService.sendViewerSignal(user.id, RC_OP_STOP)
          webrtcService.sendViewerSignal(user.id, RC_OP_RESET)
          setRemoteControlState({ isControlling: false, isCursorHidden: false })
          setIsCursorHidden(false)
          try {
            window.windowControls.remoteControlUnclipCursor()
          } catch { }
          if (document.pointerLockElement) {
            try { document.exitPointerLock() } catch { }
          }
        }
        return
      }

      if (isRemoteControlling) {
        if (e.key === 'Escape' || e.code === 'Escape') {
          e.preventDefault()
          e.stopPropagation()
          e.stopImmediatePropagation?.()
          const nextHidden = !isCursorHidden
          setIsCursorHidden(nextHidden)
          setRemoteControlState({ isCursorHidden: nextHidden })
          if (nextHidden) {
            if (videoRef.current) {
              const video = videoRef.current
              const vRect = video.getBoundingClientRect()
              const rRect = getVideoRenderRect(video)
              const scale = window.devicePixelRatio || 1
              const screenLeft = Math.round((window.screenX + vRect.left + rRect.left) * scale)
              const screenTop = Math.round((window.screenY + vRect.top + rRect.top) * scale)
              const screenRight = Math.round((window.screenX + vRect.left + rRect.left + rRect.width) * scale)
              const screenBottom = Math.round((window.screenY + vRect.top + rRect.top + rRect.height) * scale)
              try {
                window.windowControls.remoteControlClipCursor({
                  left: screenLeft,
                  top: screenTop,
                  right: screenRight,
                  bottom: screenBottom
                })
              } catch { }
            }
          } else {
            try {
              window.windowControls.remoteControlUnclipCursor()
            } catch { }
          }
          const msg = nextHidden ? t('stream.cursorHidden', 'курсор скрыт') : t('stream.cursorShown', 'курсор показан')
          useAppStore.getState().setSystemToast(msg)
          setTimeout(() => {
            if (useAppStore.getState().systemToast === msg) {
              useAppStore.getState().setSystemToast(null)
            }
          }, 2000)
          return
        }

        e.preventDefault()
        e.stopPropagation()
        e.stopImmediatePropagation?.()

        if (!isHostActive) {
          const vk = getWinVkCode(e)
          const modifiers = {
            alt: e.altKey,
            ctrl: e.ctrlKey,
            shift: e.shiftKey,
            win: e.metaKey
          }
          if (isShortcutSafe(vk, modifiers)) {
            let modBits = 0
            if (e.shiftKey) modBits |= 1
            if (e.ctrlKey) modBits |= 2
            if (e.altKey) modBits |= 4
            if (e.metaKey) modBits |= 8
            webrtcService.sendViewerKey(user.id, vk, true, modBits)
          }
        }
        return
      }
    }

    const handleKeyUp = (e: KeyboardEvent) => {
      if (isRemoteControlling) {
        if (e.key === 'Escape' || e.code === 'Escape') {
          e.preventDefault()
          e.stopPropagation()
          e.stopImmediatePropagation?.()
          return
        }

        if (e.ctrlKey && e.altKey && (e.code === 'KeyK' || e.key.toLowerCase() === 'k')) {
          return
        }

        e.preventDefault()
        e.stopPropagation()
        e.stopImmediatePropagation?.()

        if (!isHostActive) {
          const vk = getWinVkCode(e)
          const modifiers = {
            alt: e.altKey,
            ctrl: e.ctrlKey,
            shift: e.shiftKey,
            win: e.metaKey
          }
          if (isShortcutSafe(vk, modifiers)) {
            let modBits = 0
            if (e.shiftKey) modBits |= 1
            if (e.ctrlKey) modBits |= 2
            if (e.altKey) modBits |= 4
            if (e.metaKey) modBits |= 8
            webrtcService.sendViewerKey(user.id, vk, false, modBits)
          }
        }
        return
      }
    }

    window.addEventListener('keydown', handleKeyDown, { capture: true })
    window.addEventListener('keyup', handleKeyUp, { capture: true })
    return () => {
      window.removeEventListener('keydown', handleKeyDown, { capture: true })
      window.removeEventListener('keyup', handleKeyUp, { capture: true })
    }
  }, [isFullscreen, isLocal, user.id, isRemoteControlling, isHostActive, isRemoteControlRevoked, setRemoteControlState, isCursorHidden, t])

  useEffect(() => {
    if (!isFullscreen || isLocal || !isRemoteControlling) return
    const timer = setInterval(() => {
      webrtcService.sendViewerSignal(user.id, RC_OP_PING)
    }, 1500)
    return () => clearInterval(timer)
  }, [isFullscreen, isLocal, isRemoteControlling, user.id])

  useEffect(() => {
    return () => {
      if (moveThrottleTimer.current) {
        clearTimeout(moveThrottleTimer.current)
        moveThrottleTimer.current = null
      }
      try {
        window.windowControls.remoteControlUnclipCursor()
      } catch { }
    }
  }, [isRemoteControlling])

  useEffect(() => {
    const handleBlur = () => {
      try {
        window.windowControls.remoteControlUnclipCursor()
      } catch { }
      setIsCursorHidden(false)
      setRemoteControlState({ isCursorHidden: false })
    }
    window.addEventListener('blur', handleBlur)
    return () => window.removeEventListener('blur', handleBlur)
  }, [setRemoteControlState])

  const handleMouseMove = (e: React.MouseEvent<HTMLVideoElement>) => {
    if (!isRemoteControlling || isHostActive || !videoRef.current) return
    const video = videoRef.current
    const renderRect = getVideoRenderRect(video)
    if (renderRect.width <= 0 || renderRect.height <= 0) return

    let xNorm = 0
    let yNorm = 0

    if (document.pointerLockElement === video) {
      virtualPos.current.x = Math.max(0, Math.min(1, virtualPos.current.x + e.movementX / renderRect.width))
      virtualPos.current.y = Math.max(0, Math.min(1, virtualPos.current.y + e.movementY / renderRect.height))
      xNorm = virtualPos.current.x
      yNorm = virtualPos.current.y
    } else {
      const rect = video.getBoundingClientRect()
      const clientX = e.clientX - rect.left - renderRect.left
      const clientY = e.clientY - rect.top - renderRect.top
      xNorm = Math.max(0, Math.min(1, clientX / renderRect.width))
      yNorm = Math.max(0, Math.min(1, clientY / renderRect.height))
      virtualPos.current = { x: xNorm, y: yNorm }
    }

    const now = performance.now()
    if (now - lastMoveTime.current >= 4) {
      lastMoveTime.current = now
      webrtcService.sendViewerMove(user.id, xNorm, yNorm)
      if (moveThrottleTimer.current) {
        clearTimeout(moveThrottleTimer.current)
        moveThrottleTimer.current = null
      }
      pendingMove.current = null
    } else {
      pendingMove.current = { x: xNorm, y: yNorm }
      if (!moveThrottleTimer.current) {
        moveThrottleTimer.current = setTimeout(() => {
          moveThrottleTimer.current = null
          if (pendingMove.current) {
            lastMoveTime.current = performance.now()
            webrtcService.sendViewerMove(user.id, pendingMove.current.x, pendingMove.current.y)
            pendingMove.current = null
          }
        }, 4)
      }
    }
  }

  const handleMouseDown = (e: React.MouseEvent<HTMLVideoElement>) => {
    if (!isRemoteControlling || isHostActive) return
    e.preventDefault()
    e.stopPropagation()
    if (moveThrottleTimer.current) {
      clearTimeout(moveThrottleTimer.current)
      moveThrottleTimer.current = null
    }
    if (pendingMove.current) {
      webrtcService.sendViewerMove(user.id, pendingMove.current.x, pendingMove.current.y)
      pendingMove.current = null
    }
    webrtcService.sendViewerMouseButton(user.id, e.button, true)
  }

  const handleMouseUp = (e: React.MouseEvent<HTMLVideoElement>) => {
    if (!isRemoteControlling || isHostActive) return
    e.preventDefault()
    e.stopPropagation()
    if (moveThrottleTimer.current) {
      clearTimeout(moveThrottleTimer.current)
      moveThrottleTimer.current = null
    }
    if (pendingMove.current) {
      webrtcService.sendViewerMove(user.id, pendingMove.current.x, pendingMove.current.y)
      pendingMove.current = null
    }
    webrtcService.sendViewerMouseButton(user.id, e.button, false)
  }

  const handleVideoContextMenu = (e: React.MouseEvent<HTMLVideoElement>) => {
    if (isRemoteControlling && !isHostActive) {
      e.preventDefault()
      e.stopPropagation()
    }
  }

  const handleWheel = (e: React.WheelEvent<HTMLVideoElement>) => {
    if (!isRemoteControlling || isHostActive) return
    e.preventDefault()
    e.stopPropagation()
    webrtcService.sendViewerMouseWheel(user.id, Math.round(e.deltaY), Math.round(e.deltaX))
  }

  useEffect(() => {
    if (isLocal || !stream) return
    const videoTrack = stream.getVideoTracks()[0]
    if (!videoTrack || videoTrack.readyState === 'ended') return
    const isWatching = isFocused || isFullscreen
    if (isWatching) {
      videoTrack.enabled = true
    } else {
      videoTrack.enabled = isCapturing
    }
    return () => {
      if (videoTrack.readyState !== 'ended') {
        videoTrack.enabled = true
      }
    }
  }, [stream, isFocused, isFullscreen, isCapturing, isLocal])

  useEffect(() => {
    if (mode !== 'normal') {
      if (captureTimeoutRef.current) {
        clearTimeout(captureTimeoutRef.current)
        captureTimeoutRef.current = null
      }
      if (retryTimerRef.current) {
        clearTimeout(retryTimerRef.current)
        retryTimerRef.current = null
      }
      setIsCapturing(false)
      return
    }

    const captureFrame = () => {
      setIsCapturing(true)
    }

    captureFrame()
    const timer = setInterval(captureFrame, 60000)
    return () => {
      clearInterval(timer)
      if (retryTimerRef.current) {
        clearTimeout(retryTimerRef.current)
        retryTimerRef.current = null
      }
      if (captureTimeoutRef.current) {
        clearTimeout(captureTimeoutRef.current)
        captureTimeoutRef.current = null
      }
    }
  }, [mode, stream])

  useEffect(() => {
    if (isCapturing && captureVideoRef.current && stream) {
      const video = captureVideoRef.current
      video.srcObject = stream

      const handlePlay = () => {
        if (captureTimeoutRef.current) clearTimeout(captureTimeoutRef.current)
        captureTimeoutRef.current = setTimeout(() => {
          captureTimeoutRef.current = null
          try {
            if (!video || video.readyState < 2 || video.videoWidth === 0 || video.videoHeight === 0) {
              setIsCapturing(false)
              if (video?.srcObject) video.srcObject = null
              return
            }
            const canvas = document.createElement('canvas')
            canvas.width = 320
            canvas.height = 180
            const ctx = canvas.getContext('2d', { willReadFrequently: true })
            if (ctx) {
              ctx.drawImage(video, 0, 0, canvas.width, canvas.height)
              const imgData = ctx.getImageData(0, 0, canvas.width, canvas.height).data
              let isBlack = true
              for (let i = 0; i < imgData.length; i += 40) {
                if (imgData[i] > 15 || imgData[i + 1] > 15 || imgData[i + 2] > 15) {
                  isBlack = false
                  break
                }
              }
              if (!isBlack) {
                const dataUrl = canvas.toDataURL('image/jpeg', 0.6)
                setSnapshot(dataUrl)
              } else if (!snapshot) {
                if (retryTimerRef.current) clearTimeout(retryTimerRef.current)
                retryTimerRef.current = setTimeout(() => {
                  setIsCapturing(true)
                }, 2000)
              }
            }
          } catch (e) {
            console.error(e)
          } finally {
            setIsCapturing(false)
            if (video?.srcObject) {
              video.srcObject = null
            }
          }
        }, isLocal ? 150 : 2000)
      }

      video.addEventListener('playing', handlePlay, { once: true })
      return () => {
        video.removeEventListener('playing', handlePlay)
        if (captureTimeoutRef.current) {
          clearTimeout(captureTimeoutRef.current)
          captureTimeoutRef.current = null
        }
        if (video.srcObject) {
          video.srcObject = null
        }
      }
    }
  }, [isCapturing, stream, snapshot, isLocal])

  useEffect(() => {
    if (!stream) setHasFirstFrame(false)
  }, [stream])

  const videoOnlyStream = useMemo(() => {
    const tracks = stream ? stream.getVideoTracks() : []
    return tracks.length > 0 ? new MediaStream(tracks) : null
  }, [stream])

  useEffect(() => {
    if (mode === 'normal' || !videoRef.current) return
    const video = videoRef.current
    if (!videoOnlyStream) {
      video.srcObject = null
      return
    }
    if (video.srcObject !== videoOnlyStream) video.srcObject = videoOnlyStream
    if ('playoutDelayHint' in video) {
      try { (video as any).playoutDelayHint = 0 } catch { }
    }
    if (!isLocal) {
      webrtcService.enforceVideoZeroPlayoutDelay(user.id)
    }

    let cancelled = false
    let frameHandle = 0
    const markReady = () => {
      if (!cancelled) setHasFirstFrame(true)
    }

    const anyVideo = video as HTMLVideoElement & {
      requestVideoFrameCallback?: (callback: () => void) => number
      cancelVideoFrameCallback?: (handle: number) => void
    }

    if (typeof anyVideo.requestVideoFrameCallback === 'function') {
      frameHandle = anyVideo.requestVideoFrameCallback(markReady)
    } else {
      const onPlaying = () => {
        if (video.videoWidth > 0) markReady()
      }
      video.addEventListener('playing', onPlaying)
      video.addEventListener('loadeddata', onPlaying)
      return () => {
        cancelled = true
        video.removeEventListener('playing', onPlaying)
        video.removeEventListener('loadeddata', onPlaying)
        if (video) {
          try { video.pause() } catch { }
          video.srcObject = null
        }
      }
    }

    return () => {
      cancelled = true
      if (frameHandle && typeof anyVideo.cancelVideoFrameCallback === 'function') {
        anyVideo.cancelVideoFrameCallback(frameHandle)
      }
      if (video) {
        try { video.pause() } catch { }
        video.srcObject = null
      }
    }
  }, [mode, videoOnlyStream])

  useLayoutEffect(() => {
    const el = rootRef.current
    if (!el) return
    if (mode === 'fullscreen' || prefersReducedMotion()) {
      utils.set(el, { opacity: 1, scale: 1 })
      return
    }
    utils.set(el, { opacity: 0, scale: 0.9 })
    animate(el, { opacity: [0, 1], scale: [0.9, 1], ease: spring({ stiffness: 300, damping: 25 }) })
  }, [])

  if (mode === 'normal') {
    return (
      <div
        ref={rootRef}
        onContextMenu={onContextMenu}
        onClick={onClick}
        className="relative flex flex-col overflow-hidden cursor-pointer bg-transparent group transition-all duration-300"
        style={{
          width: `${cardSize.w}px`,
          height: `${cardSize.h}px`,
          borderRadius: '12px',
          WebkitTransform: 'translate3d(0, 0, 0)',
          transform: 'translate3d(0, 0, 0)'
        }}
      >
        <div className="absolute inset-[1.5px] overflow-hidden bg-[#0B0B0F] flex items-center justify-center rounded-[10.5px]">
          {snapshot ? (
            <img
              src={snapshot}
              className="w-full h-full object-cover filter blur-[4px] scale-[1.05] transition-all duration-500"
            />
          ) : (
            <div className="absolute inset-0 bg-[#0B0B0F]" />
          )}
          <div className="absolute inset-0 bg-black/40" />
        </div>

        {!snapshot && <StreamLoadingDots compact={compactPill} label={t('stream.loading')} />}

        <StreamOwnerPill user={user} cardWidth={cardSize.w} compact={compactPill} />

        <div className="absolute inset-0 rounded-xl ring-1 ring-inset ring-[#303035] group-hover:ring-2 group-hover:ring-primaryHover group-hover:ring-inset pointer-events-none z-20 transition-all duration-300" />

        {isCapturing && (
          <video
            ref={captureVideoRef}
            autoPlay
            muted
            playsInline
            style={{ display: 'none' }}
          />
        )}
      </div>
    )
  }

  return (
    <div
      ref={rootRef}
      onContextMenu={onContextMenu}
      style={{
        WebkitTransform: 'translate3d(0, 0, 0)',
        transform: 'translate3d(0, 0, 0)'
      }}
      className={`relative overflow-hidden group ${isFullscreen ? 'w-full h-full bg-[#0B0B0F] rounded-none' : 'w-full h-full bg-transparent rounded-xl'
        } ${isRemoteControlling && !isHostActive ? (isCursorHidden ? 'cursor-none select-none' : 'cursor-default select-none') : ''}`}
    >
      <div className={`${isFullscreen ? "w-full h-full bg-black" : "absolute inset-[1.5px] overflow-hidden rounded-[10.5px] bg-[#0B0B0F]"} ${isRemoteControlling && !isHostActive ? (isCursorHidden ? 'cursor-none select-none' : 'cursor-default select-none') : ''}`}>
        <video
          ref={videoRef}
          autoPlay
          playsInline
          muted
          onLoadedMetadata={(e) => {
            const video = e.currentTarget
            if ('playoutDelayHint' in video) {
              try { (video as any).playoutDelayHint = 0 } catch { }
            }
            if (!isLocal) {
              webrtcService.enforceVideoZeroPlayoutDelay(user.id)
            }
            if (video.videoWidth && video.videoHeight && onRatioChange) {
              onRatioChange(video.videoWidth / video.videoHeight)
            }
          }}
          onPlaying={(e) => {
            const video = e.currentTarget
            if ('playoutDelayHint' in video) {
              try { (video as any).playoutDelayHint = 0 } catch { }
            }
            if (!isLocal) {
              webrtcService.enforceVideoZeroPlayoutDelay(user.id)
            }
          }}
          onMouseMove={isFullscreen && isRemoteControlling ? handleMouseMove : undefined}
          onMouseDown={isFullscreen && isRemoteControlling ? handleMouseDown : undefined}
          onMouseUp={isFullscreen && isRemoteControlling ? handleMouseUp : undefined}
          onWheel={isFullscreen && isRemoteControlling ? handleWheel : undefined}
          onContextMenu={isFullscreen && isRemoteControlling ? handleVideoContextMenu : undefined}
          className={`w-full h-full object-contain ${
            isRemoteControlling && !isHostActive
              ? (isCursorHidden ? 'cursor-none select-none' : 'cursor-default select-none')
              : ''
          }`}
        />
      </div>

      {isFullscreen && isRemoteControlling && (
        <div
          className={`absolute inset-0 pointer-events-none z-30 transition-colors duration-200 ${
            isHostActive
              ? 'ring-2 ring-inset ring-white/20'
              : 'ring-2 ring-inset ring-primary'
          }`}
        />
      )}

      {!hasFirstFrame && (
        <>
          <StreamLoadingDots compact={false} label={t('stream.loading')} />
          <StreamOwnerPill user={user} cardWidth={cardSize.w} compact={false} />
        </>
      )}

      {!isFullscreen && (
        <div className="absolute inset-0 rounded-xl ring-1 ring-inset ring-[#303035] pointer-events-none z-20" />
      )}

      <div className={`absolute inset-x-0 top-0 p-4 pointer-events-none flex items-center justify-between z-10 transition-all duration-300 ${isFullscreen && !showOverlays ? '-translate-y-4 opacity-0' : 'translate-y-0 opacity-100'}`}>
        <div className="bg-[#09090B]/85 border border-[#303035]/50 px-3 py-1 rounded-full flex items-center gap-2">
          <span className="text-white font-bold text-xs truncate">
            {t('stream.broadcastOf', { name: user.displayName })}
          </span>
        </div>

        <div className="flex items-center gap-2">
          <span className="bg-[#09090B]/85 border border-[#303035]/50 text-textMuted text-[10px] font-black px-2 py-0.5 rounded-md">
            {user.streamQuality ? user.streamQuality.toLowerCase() : 'low'}
          </span>
        </div>
      </div>

      {!isFullscreen && (
        <>
          <div className="absolute inset-0 z-10 pointer-events-none flex items-end justify-start p-4">
            <button
              onClick={(e) => {
                e.stopPropagation()
                if (onClick) onClick()
              }}
              style={{ willChange: 'transform, opacity' }}
              className="group/mode-button relative w-10 h-10 flex items-center justify-center bg-[#09090B]/70 backdrop-blur-xl border border-primaryHover/30 border-t-primaryHover/50 rounded-full hover:scale-110 active:scale-95 transition-[transform,opacity] duration-200 ease-out cursor-pointer pointer-events-auto opacity-0 group-hover:opacity-100 translate-y-2 group-hover:translate-y-0"
              aria-label={t('stream.minimizeHint')}
            >
              <span
                role="tooltip"
                className="absolute bottom-full left-0 mb-2 pointer-events-none whitespace-nowrap opacity-0 group-hover/mode-button:opacity-100 delay-0 group-hover/mode-button:delay-[2000ms] transition-opacity duration-150 bg-[#09090B]/80 backdrop-blur-md border border-white/[0.07] border-t-white/[0.14] rounded-md px-2 py-1 text-[9px] font-bold text-white"
              >
                {t('stream.minimizeHint')}
              </span>
              <CornersIn weight="bold" size={20} className="text-primaryHover shrink-0" />
            </button>
          </div>

          <div className="absolute inset-0 z-10 pointer-events-none flex items-end justify-end p-4">
            <button
              onClick={(e) => {
                e.stopPropagation()
                if (onToggleFullscreen) onToggleFullscreen()
              }}
              style={{ willChange: 'transform, opacity' }}
              className="group/mode-button relative w-10 h-10 flex items-center justify-center bg-[#09090B]/70 backdrop-blur-xl border border-primaryHover/30 border-t-primaryHover/50 rounded-full hover:scale-110 active:scale-95 transition-[transform,opacity] duration-200 ease-out cursor-pointer pointer-events-auto opacity-0 group-hover:opacity-100 translate-y-2 group-hover:translate-y-0"
              aria-label={t('stream.fullscreenHint')}
            >
              <span
                role="tooltip"
                className="absolute bottom-full right-0 mb-2 pointer-events-none whitespace-nowrap opacity-0 group-hover/mode-button:opacity-100 delay-0 group-hover/mode-button:delay-[2000ms] transition-opacity duration-150 bg-[#09090B]/80 backdrop-blur-md border border-white/[0.07] border-t-white/[0.14] rounded-md px-2 py-1 text-[9px] font-bold text-white"
              >
                {t('stream.fullscreenHint')}
              </span>
              <CornersOut weight="bold" size={20} className="text-primaryHover shrink-0" />
            </button>
          </div>
        </>
      )}
    </div>
  )
}
