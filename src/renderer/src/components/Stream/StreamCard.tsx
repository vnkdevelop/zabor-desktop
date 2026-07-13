import { useEffect, useRef, useState } from 'react'
import { motion } from 'framer-motion'
import { CornersIn, CornersOut } from '@phosphor-icons/react'
import { useTranslation } from 'react-i18next'
import { User, useAppStore } from '../../store/useAppStore'

interface StreamCardProps {
  user: User
  stream: MediaStream
  cardSize: { w: number; h: number }
  isFocused: boolean
  isFullscreen?: boolean
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

  const currentUserId = useAppStore((state) => state.currentUser?.id)
  const isLocal = user.id === currentUserId

  const mode = isFullscreen ? 'fullscreen' : (isFocused ? 'focused' : 'normal')

  useEffect(() => {
    if (mode !== 'normal') return

    const captureFrame = () => {
      setIsCapturing(true)
    }

    captureFrame()
    const timer = setInterval(captureFrame, 60000)
    return () => clearInterval(timer)
  }, [mode, stream])

  useEffect(() => {
    if (isCapturing && captureVideoRef.current) {
      const video = captureVideoRef.current
      video.srcObject = stream

      const handlePlay = () => {
        setTimeout(() => {
          try {
            const canvas = document.createElement('canvas')
            canvas.width = 320
            canvas.height = 180
            const ctx = canvas.getContext('2d')
            if (ctx) {
              ctx.drawImage(video, 0, 0, canvas.width, canvas.height)
              const dataUrl = canvas.toDataURL('image/jpeg', 0.6)
              setSnapshot(dataUrl)
            }
          } catch (e) {
            console.error(e)
          } finally {
            setIsCapturing(false)
            if (video.srcObject) {
              video.srcObject = null
            }
          }
        }, 1000)
      }

      video.addEventListener('playing', handlePlay, { once: true })
      return () => {
        video.removeEventListener('playing', handlePlay)
      }
    }
  }, [isCapturing, stream])

  useEffect(() => {
    if (mode !== 'normal' && videoRef.current) {
      videoRef.current.srcObject = stream
    }
  }, [mode, stream])

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
          WebkitMaskImage: '-webkit-radial-gradient(white, black)',
          maskImage: 'radial-gradient(white, black)'
        }}
      >
        <div className="absolute inset-[1.5px] overflow-hidden bg-[#0B0B0F] flex items-center justify-center rounded-[10.5px]">
          {snapshot ? (
            <img
              src={snapshot}
              className="w-full h-full object-cover filter blur-[4px] scale-[1.05] transition-all duration-500"
            />
          ) : (
            <div className="absolute inset-0 bg-[#0B0B0F] flex items-center justify-center text-textMuted text-xs font-bold">
              {t('stream.loadingFrame', 'Подключение...')}
            </div>
          )}

          <div className="absolute inset-0 flex flex-col items-center justify-center bg-black/40">
            <span className="text-white text-sm font-black tracking-wide text-center drop-shadow-md">
              Трансляция {user.displayName}
            </span>
          </div>
        </div>

        <div className="absolute inset-0 rounded-xl ring-1 ring-inset ring-[#303035] group-hover:ring-2 group-hover:ring-[#FF007F] group-hover:ring-inset pointer-events-none z-20 transition-all duration-300" />

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
        WebkitMaskImage: '-webkit-radial-gradient(white, black)',
        maskImage: 'radial-gradient(white, black)'
      }}
      className={`relative overflow-hidden group ${isFullscreen ? 'w-full h-full bg-[#0B0B0F] rounded-none' : 'w-full h-full bg-transparent rounded-xl'
        }`}
    >
      <div className={isFullscreen ? "w-full h-full bg-black" : "absolute inset-[1.5px] overflow-hidden rounded-[10.5px] bg-[#0B0B0F]"}>
        <video
          ref={videoRef}
          autoPlay
          playsInline
          muted={isLocal}
          onLoadedMetadata={(e) => {
            const video = e.currentTarget
            if (video.videoWidth && video.videoHeight && onRatioChange) {
              onRatioChange(video.videoWidth / video.videoHeight)
            }
          }}
          className="w-full h-full object-contain"
        />
      </div>

      {!isFullscreen && (
        <div className="absolute inset-0 rounded-xl ring-1 ring-inset ring-[#303035] group-hover:ring-2 group-hover:ring-[#FF007F] group-hover:ring-inset pointer-events-none z-20 transition-all duration-300" />
      )}

      <div className="absolute inset-x-0 top-0 p-4 bg-gradient-to-b from-black/80 to-transparent pointer-events-none flex items-center justify-between z-10">
        <div className="bg-[#09090B]/85 border border-[#303035]/50 px-3 py-1 rounded-full flex items-center gap-2">
          <span className="text-white font-bold text-xs truncate">
            Трансляция {user.displayName}
          </span>
        </div>

        <div className="flex items-center gap-2">
          <span className="bg-[#FF007F] text-white text-[10px] font-black tracking-wider px-2 py-0.5 rounded-md animate-pulse">
            LIVE
          </span>
          <span className="bg-[#09090B]/85 border border-[#303035]/50 text-textMuted text-[10px] font-black px-2 py-0.5 rounded-md">
            {user.streamQuality || '720p'}
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
              className="w-10 h-10 flex items-center justify-center bg-[#09090B]/90 border border-[#FF007F]/30 rounded-full hover:scale-110 active:scale-95 transition-[transform,opacity] duration-200 ease-out cursor-pointer pointer-events-auto opacity-0 group-hover:opacity-100 translate-y-2 group-hover:translate-y-0"
              title={t('stream.minimize', 'Свернуть трансляцию')}
            >
              <CornersIn weight="bold" size={20} className="text-[#FF007F] shrink-0" />
            </button>
          </div>

          <div className="absolute inset-0 z-10 pointer-events-none flex items-end justify-end p-4">
            <button
              onClick={(e) => {
                e.stopPropagation()
                if (onToggleFullscreen) onToggleFullscreen()
              }}
              style={{ willChange: 'transform, opacity' }}
              className="w-10 h-10 flex items-center justify-center bg-[#09090B]/90 border border-[#FF007F]/30 rounded-full hover:scale-110 active:scale-95 transition-[transform,opacity] duration-200 ease-out cursor-pointer pointer-events-auto opacity-0 group-hover:opacity-100 translate-y-2 group-hover:translate-y-0"
            >
              <CornersOut weight="bold" size={20} className="text-[#FF007F] shrink-0" />
            </button>
          </div>
        </>
      )}
    </motion.div>
  )
}
