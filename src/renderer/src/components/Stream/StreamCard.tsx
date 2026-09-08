import { useEffect, useMemo, useRef, useState } from 'react'
import { motion } from 'framer-motion'
import { CornersIn, CornersOut } from '@phosphor-icons/react'
import { useTranslation } from 'react-i18next'
import { User, useAppStore } from '../../store/useAppStore'
import { AvatarImg } from '../Shared/AvatarImg'

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
  stream: MediaStream
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

  useEffect(() => {
    if (isLocal) return
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
    if (isCapturing && captureVideoRef.current) {
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
    setHasFirstFrame(false)
  }, [stream])

  const videoOnlyStream = useMemo(() => {
    const tracks = stream.getVideoTracks()
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

  if (mode === 'normal') {
    return (
      <motion.div
        initial={{ scale: 0.9, opacity: 0 }}
        animate={{ scale: 1, opacity: 1 }}
        transition={{ type: 'spring', stiffness: 300, damping: 25 }}
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
      </motion.div>
    )
  }

  return (
    <motion.div
      initial={isFullscreen ? {} : { scale: 0.9, opacity: 0 }}
      animate={isFullscreen ? {} : { scale: 1, opacity: 1 }}
      transition={{ type: 'spring', stiffness: 300, damping: 25 }}
      onContextMenu={onContextMenu}
      style={{
        WebkitTransform: 'translate3d(0, 0, 0)',
        transform: 'translate3d(0, 0, 0)'
      }}
      className={`relative overflow-hidden group ${isFullscreen ? 'w-full h-full bg-[#0B0B0F] rounded-none' : 'w-full h-full bg-transparent rounded-xl'
        }`}
    >
      <div className={isFullscreen ? "w-full h-full bg-black" : "absolute inset-[1.5px] overflow-hidden rounded-[10.5px] bg-[#0B0B0F]"}>
        <video
          ref={videoRef}
          autoPlay
          playsInline
          muted
          onLoadedMetadata={(e) => {
            const video = e.currentTarget
            if (video.videoWidth && video.videoHeight && onRatioChange) {
              onRatioChange(video.videoWidth / video.videoHeight)
            }
          }}
          className="w-full h-full object-contain"
        />
      </div>

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
            {user.streamQuality ? (user.streamQuality === 'high' ? 'High' : (user.streamQuality === 'low' ? 'Low' : user.streamQuality)) : 'Low'}
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
    </motion.div>
  )
}
