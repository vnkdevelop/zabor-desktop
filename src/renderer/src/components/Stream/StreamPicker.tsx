import { useState, useEffect } from 'react'
import { X, Desktop, AppWindow } from '@phosphor-icons/react'
import { useTranslation } from 'react-i18next'

interface Source {
  id: string
  name: string
  thumbnail: string
  appIcon: string | null
}

interface StreamPickerProps {
  onClose: () => void
  onSelect: (sourceId: string, quality: '1080p' | '720p') => void
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
  const [activeTab, setActiveTab] = useState<'screen' | 'window'>('screen')
  const [quality, setQuality] = useState<'1080p' | '720p'>('720p')
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    let active = true
    const fetchSources = async () => {
      setLoading(true)
      try {
        const res = await (window as any).windowControls.getDesktopSources({
          types: [activeTab],
          thumbnailSize: { width: 220, height: 150 },
          fetchWindowIcons: true
        })
        if (active) {
          setSources(res)
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

  return (
    <div className="bg-[#161618] border border-[#303035] rounded-3xl p-6 w-[540px] max-w-full shadow-2xl flex flex-col h-[500px]">
      <div className="flex items-center justify-between mb-6">
        <h2 className="text-white text-xl font-bold">{t('stream.pickerTitle', 'Выбор источника')}</h2>
        <button
          onClick={onClose}
          className="text-textMuted hover:text-white transition-all duration-200 hover:rotate-90 hover:scale-110 p-1.5 rounded-lg hover:bg-surface"
        >
          <X weight="bold" size={20} />
        </button>
      </div>

      <div className="flex gap-4 mb-6 border-b border-[#303035] pb-3">
        <button
          onClick={() => setActiveTab('screen')}
          className={`flex items-center gap-2 pb-2 px-1 font-bold text-sm transition-all relative ${
            activeTab === 'screen' ? 'text-[#FF007F]' : 'text-textMuted hover:text-white'
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
          className={`flex items-center gap-2 pb-2 px-1 font-bold text-sm transition-all relative ${
            activeTab === 'window' ? 'text-[#FF007F]' : 'text-textMuted hover:text-white'
          }`}
        >
          <AppWindow weight="bold" size={16} />
          {t('stream.apps', 'Приложения')}
          {activeTab === 'window' && (
            <div className="absolute bottom-0 left-0 right-0 h-0.5 bg-[#FF007F] rounded-full" />
          )}
        </button>

        <div className="ml-auto flex items-center bg-[#0B0B0F] border border-[#303035] rounded-xl p-0.5 relative shrink-0">
          <button
            onClick={() => setQuality('720p')}
            className={`px-3 py-1 rounded-lg text-xs font-bold z-10 transition-all duration-200 hover:scale-[1.02] active:scale-[0.98] ${
              quality === '720p' ? 'text-white' : 'text-textMuted hover:text-white'
            }`}
          >
            720p
          </button>
          <button
            onClick={() => setQuality('1080p')}
            className={`px-3 py-1 rounded-lg text-xs font-bold z-10 transition-all duration-200 hover:scale-[1.02] active:scale-[0.98] ${
              quality === '1080p' ? 'text-white' : 'text-textMuted hover:text-white'
            }`}
          >
            1080p
          </button>
          <div
            style={{
              transform: quality === '720p' ? 'translateX(0)' : 'translateX(calc(100% + 2px))',
              willChange: 'transform'
            }}
            className="absolute top-0.5 bottom-0.5 left-0.5 w-[calc(50%-3px)] bg-[#FF007F] rounded-lg transition-transform duration-300 ease-out"
          />
        </div>
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
          sources.map((src, index) => (
            <button
              key={src.id}
              onClick={() => onSelect(src.id, quality)}
              className="group bg-[#0B0B0F] border border-[#303035] hover:border-[#FF007F] rounded-2xl p-3 flex flex-col items-stretch text-left transition-all duration-200 hover:-translate-y-0.5"
            >
              <div className="relative aspect-video rounded-lg overflow-hidden bg-black mb-3">
                <img
                  src={src.thumbnail}
                  alt={src.name}
                  className="w-full h-full object-cover group-hover:scale-105 transition-transform duration-300"
                />
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
          ))
        )}
      </div>
    </div>
  )
}
