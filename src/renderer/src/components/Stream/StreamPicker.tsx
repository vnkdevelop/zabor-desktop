import React, { useState, useEffect } from 'react'
import { X, Desktop, AppWindow, Camera } from '@phosphor-icons/react'
import { useTranslation } from 'react-i18next'
import { motion, AnimatePresence } from 'framer-motion'

interface Source {
  id: string
  name: string
  thumbnail: string
  appIcon: string | null
}

interface StreamPickerProps {
  onClose: () => void
  onSelect: (sourceId: string, quality: 'low' | 'high' | 'camera', includeAudio: boolean) => void
}

const cleanName = (srcName: string, index: number, isScreen: boolean) => {
  if (isScreen) {
    return `Экран ${index + 1}`
  }
  const parts = srcName.split(' - ')
  if (parts.length > 1) {
    return parts[parts.length - 1]
  }
  return srcName
}

export const StreamPicker = ({ onClose, onSelect }: StreamPickerProps) => {
  const { t } = useTranslation()
  const [sources, setSources] = useState<Source[]>([])
  const [activeTab, setActiveTab] = useState<'screen' | 'window' | 'camera'>('screen')
  const [selectedSourceId, setSelectedSourceId] = useState<string | null>(null)
  const [quality, setQuality] = useState<'low' | 'high'>('low')
  const [includeAudio, setIncludeAudio] = useState(true)
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    setSelectedSourceId(null)
  }, [activeTab])

  useEffect(() => {
    let active = true
    const fetchSources = async () => {
      setLoading(true)
      try {
        if (activeTab === 'camera') {
          try {
            const tempStream = await navigator.mediaDevices.getUserMedia({ video: true })
            tempStream.getTracks().forEach(track => track.stop())
          } catch { }
          const devices = await navigator.mediaDevices.enumerateDevices()
          const videoDevices = devices.filter(d => d.kind === 'videoinput')
          const mapped: Source[] = videoDevices.map((d, idx) => ({
            id: `camera:${d.deviceId}`,
            name: d.label || `Камера ${idx + 1}`,
            thumbnail: '',
            appIcon: null
          }))
          if (active) {
            setSources(mapped)
          }
        } else {
          const res = await (window as any).windowControls.getDesktopSources({
            types: [activeTab],
            thumbnailSize: { width: 220, height: 150 },
            fetchWindowIcons: true
          })
          if (active) {
            setSources(res)
          }
        }
      } catch (e) {
        console.error(e)
      } finally {
        if (active) {
          setLoading(false)
        }
      }
    }

    fetchSources()
    const interval = setInterval(fetchSources, 4000)

    return () => {
      active = false
      clearInterval(interval)
    }
  }, [activeTab])

  const getQualityDetailsText = () => {
    if (quality === 'high') {
      return '1920x1080 60fps (~6 Мбит/с)'
    }
    return '1280x720 30fps (~2.5 Мбит/с)'
  }

  return (
    <div className="bg-[#161618] border border-[#303035] rounded-3xl p-6 w-[540px] max-w-full shadow-2xl flex flex-col h-[600px]">
      <div className="flex items-center justify-between mb-6">
        <h2 className="text-white text-xl font-bold">{t('stream.pickerTitle', 'Выбор источника')}</h2>
        <button
          onClick={onClose}
          className="text-textMuted hover:text-white transition-all duration-200 hover:rotate-90 hover:scale-110 p-1.5 rounded-lg hover:bg-surface"
        >
          <X weight="bold" size={20} />
        </button>
      </div>

      <div className="flex gap-4 mb-6 border-b border-[#303035] pb-3 items-center">
        <button
          onClick={() => setActiveTab('screen')}
          className={`flex items-center gap-2 pb-2 px-1 font-bold text-sm transition-all relative ${activeTab === 'screen' ? 'text-[#FF007F]' : 'text-textMuted hover:text-white'
            }`}
        >
          <Desktop weight="bold" size={16} />
          {t('stream.screens', 'Экраны')}
          {activeTab === 'screen' && (
            <div className="absolute bottom-0 left-0 right-0 h-0.5 bg-[#FF007F] rounded-full" />
          )}
        </button>
        <button
          onClick={() => setActiveTab('window')}
          className={`flex items-center gap-2 pb-2 px-1 font-bold text-sm transition-all relative ${activeTab === 'window' ? 'text-[#FF007F]' : 'text-textMuted hover:text-white'
            }`}
        >
          <AppWindow weight="bold" size={16} />
          {t('stream.apps', 'Приложения')}
          {activeTab === 'window' && (
            <div className="absolute bottom-0 left-0 right-0 h-0.5 bg-[#FF007F] rounded-full" />
          )}
        </button>
        <button
          onClick={() => setActiveTab('camera')}
          className={`flex items-center gap-2 pb-2 px-1 font-bold text-sm transition-all relative ${activeTab === 'camera' ? 'text-[#FF007F]' : 'text-textMuted hover:text-white'
            }`}
        >
          <Camera weight="bold" size={16} />
          {t('stream.cameras', 'Камеры')}
          {activeTab === 'camera' && (
            <div className="absolute bottom-0 left-0 right-0 h-0.5 bg-[#FF007F] rounded-full" />
          )}
        </button>
      </div>

      <div className="flex-1 overflow-y-auto px-2 py-1 pr-1 grid grid-cols-2 gap-4 items-start content-start">
        {loading && sources.length === 0 ? (
          <div className="col-span-2 flex flex-col items-center justify-center gap-3 h-full pt-16">
            <div className="w-8 h-8 border-4 border-t-[#FF007F] border-r-transparent border-[#303035] rounded-full animate-spin" />
          </div>
        ) : sources.length === 0 ? (
          <div className="col-span-2 flex flex-col items-center justify-center h-full pt-16 text-textMuted text-sm font-medium">
            {t('stream.noSources', 'Источники не найдены')}
          </div>
        ) : (
          sources.map((src, index) => {
            const isSelected = selectedSourceId === src.id
            return (
              <React.Fragment key={src.id}>
                <button
                  type="button"
                  onClick={() => setSelectedSourceId(src.id)}
                  className={`group bg-[#0B0B0F] border rounded-2xl p-3 flex flex-col items-stretch text-left transition-all duration-200 hover:-translate-y-0.5 ${isSelected ? 'border-[#FF007F] shadow-[0_0_12px_rgba(255,0,127,0.25)]' : 'border-[#303035] hover:border-[#FF007F]'
                    }`}
                >
                  <div className="relative aspect-video rounded-lg overflow-hidden bg-black mb-3 flex items-center justify-center">
                    {src.thumbnail ? (
                      <img
                        src={src.thumbnail}
                        alt={src.name}
                        className="w-full h-full object-cover group-hover:scale-105 transition-transform duration-300"
                      />
                    ) : (
                      <div className="flex flex-col items-center gap-2 text-textMuted group-hover:text-white transition-colors">
                        <Camera weight="bold" size={32} className="text-[#FF007F]" />
                        <span className="text-[10px] font-bold tracking-wider">КАМЕРА</span>
                      </div>
                    )}
                    {src.appIcon && (
                      <img
                        src={src.appIcon}
                        className="absolute bottom-2 left-2 w-6 h-6 rounded-md bg-[#161618] p-0.5 border border-[#303035]"
                      />
                    )}
                  </div>
                  <span className="text-white text-xs font-bold truncate">
                    {cleanName(src.name, index, activeTab === 'screen')}
                  </span>
                </button>

                <AnimatePresence>
                  {isSelected && !src.id.startsWith('camera:') && (
                    <motion.div
                      initial={{ opacity: 0, height: 0 }}
                      animate={{ opacity: 1, height: 'auto' }}
                      exit={{ opacity: 0, height: 0 }}
                      transition={{ type: 'spring', stiffness: 300, damping: 24 }}
                      className="col-span-2 overflow-hidden bg-[#0B0B0F] border border-[#303035] rounded-2xl p-4 flex flex-col gap-3 my-2"
                    >
                      <div className="flex items-center justify-between">
                        <span className="text-xs font-bold text-textMuted uppercase tracking-wider">
                          {t('stream.shareAudio', 'Передавать звук?')}
                        </span>
                        <label className="flex items-center gap-2 cursor-pointer select-none">
                          <input
                            type="checkbox"
                            checked={includeAudio}
                            onChange={(e) => setIncludeAudio(e.target.checked)}
                            className="sr-only peer"
                          />
                          <div className="w-8 h-5 bg-[#161618] border border-[#303035] rounded-full relative transition-colors peer-checked:bg-[#FF007F] peer-checked:border-[#FF007F] after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-[#303035] after:rounded-full after:h-3.5 after:w-3.5 after:transition-all peer-checked:after:translate-x-3 peer-checked:after:bg-white" />
                        </label>
                      </div>

                      <div className="flex items-center justify-between">
                        <span className="text-xs font-bold text-textMuted uppercase tracking-wider">
                          {t('stream.quality', 'Качество')}
                        </span>
                        <div className="flex items-center bg-[#161618] border border-[#303035] rounded-full p-1 relative shrink-0 w-28 h-8">
                          <button
                            type="button"
                            onClick={() => setQuality('low')}
                            className={`flex-1 flex items-center justify-center h-full rounded-full text-xs font-bold z-10 transition-all duration-200 ${quality === 'low' ? 'text-white' : 'text-textMuted hover:text-white'
                              }`}
                          >
                            Low
                          </button>
                          <button
                            type="button"
                            onClick={() => setQuality('high')}
                            className={`flex-1 flex items-center justify-center h-full rounded-full text-xs font-bold z-10 transition-all duration-200 ${quality === 'high' ? 'text-white' : 'text-textMuted hover:text-white'
                              }`}
                          >
                            High
                          </button>
                          <div
                            style={{
                              transform: quality === 'low' ? 'translateX(0)' : 'translateX(100%)',
                              willChange: 'transform'
                            }}
                            className="absolute top-1 bottom-1 left-1 w-[calc(50%-4px)] bg-[#FF007F] rounded-full transition-transform duration-200 ease-out"
                          />
                        </div>
                      </div>
                    </motion.div>
                  )}
                </AnimatePresence>
              </React.Fragment>
            )
          })
        )}
      </div>

      <div className="mt-4 pt-4 border-t border-[#303035] flex flex-col items-center">
        {selectedSourceId && !selectedSourceId.startsWith('camera:') && (
          <span className="text-[11px] text-textMuted font-bold mb-3 tracking-wide text-center">
            {getQualityDetailsText()}
          </span>
        )}
        <button
          type="button"
          disabled={!selectedSourceId}
          onClick={() => {
            if (selectedSourceId) {
              const isCam = selectedSourceId.startsWith('camera:')
              onSelect(selectedSourceId, isCam ? 'camera' : quality, isCam ? false : includeAudio)
            }
          }}
          className={`w-full py-3 rounded-2xl font-bold text-sm transition-all duration-200 hover:scale-[1.01] active:scale-[0.98] ${selectedSourceId
            ? 'bg-[#FF007F] hover:bg-[#D80073] text-white shadow-[0_0_15px_rgba(255,0,127,0.35)]'
            : 'bg-[#1C1C1F] text-textMuted border border-[#303035] cursor-not-allowed'
            }`}
        >
          {t('stream.confirm', 'Поехали')}
        </button>
      </div>
    </div>
  )
}
