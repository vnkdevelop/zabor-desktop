import { useEffect, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { DownloadSimple, File, Paperclip, PaperPlaneRight, Trash, Copy, Broom, PhoneCall, X, Info } from '@phosphor-icons/react';
import { useTranslation } from 'react-i18next';
import type { User } from '../../store/useAppStore';
import { useChatStore } from '../../store/useChatStore';
import type { ChatMessage } from '../../chat/types';
import { chatPeer } from '../../services/chatPeer';

interface ChatPanelProps {
  currentUser: User;
  friend: User;
  onCall: () => void;
}

const EMPTY_MESSAGES: ChatMessage[] = [];

function formatSize(bytes: number, unit: (key: string) => string): string {
  if (bytes < 1024) return `${bytes} ${unit('chat.units.b')}`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} ${unit('chat.units.kb')}`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} ${unit('chat.units.mb')}`;
  return `${(bytes / 1024 / 1024 / 1024).toFixed(1)} ${unit('chat.units.gb')}`;
}

export function ChatPanel({ currentUser, friend, onCall }: ChatPanelProps) {
  const { t } = useTranslation();
  const storedMessages = useChatStore(state => state.messages[friend.id]);
  const messages = storedMessages ?? EMPTY_MESSAGES;
  const connected = useChatStore(state => state.connections[friend.id] ?? false);
  const [value, setValue] = useState('');
  const [dragging, setDragging] = useState(false);
  const [droppedFiles, setDroppedFiles] = useState<File[]>([]);
  const dragDepth = useRef(0);
  const [menu, setMenu] = useState<{ x: number; y: number; message: ChatMessage } | null>(null);
  const bottomRef = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => { void chatPeer.open(friend.id); }, [friend.id]);
  useEffect(() => () => chatPeer.keepWarm(friend.id), [friend.id]);
  useEffect(() => { bottomRef.current?.scrollIntoView({ block: 'end' }); }, [messages.length]);
  useEffect(() => {
    const markRead = () => { if (document.visibilityState === 'visible') void chatPeer.markVisibleAsRead(friend.id); };
    markRead();
    document.addEventListener('visibilitychange', markRead);
    window.addEventListener('focus', markRead);
    return () => {
      document.removeEventListener('visibilitychange', markRead);
      window.removeEventListener('focus', markRead);
    };
  }, [friend.id, storedMessages]);
  useEffect(() => {
    if (!menu) return;
    const safeMargin = 32;
    const handleMouseMove = (event: MouseEvent) => {
      const element = menuRef.current;
      if (!element) return;
      const rect = element.getBoundingClientRect();
      const isNear = event.clientX >= rect.left - safeMargin
        && event.clientX <= rect.right + safeMargin
        && event.clientY >= rect.top - safeMargin
        && event.clientY <= rect.bottom + safeMargin;
      if (!isNear) setMenu(null);
    };
    const handlePointerDown = (event: PointerEvent) => {
      if (menuRef.current && !menuRef.current.contains(event.target as Node)) setMenu(null);
    };
    window.addEventListener('mousemove', handleMouseMove);
    window.addEventListener('pointerdown', handlePointerDown);
    return () => {
      window.removeEventListener('mousemove', handleMouseMove);
      window.removeEventListener('pointerdown', handlePointerDown);
    };
  }, [menu]);

  const canSend = value.trim().length > 0 && value.trim().length <= 4000;
  const status = useMemo(() => connected ? t('chat.connected') : friend.isOnline ? t('chat.connecting') : t('chat.offline'), [connected, friend.isOnline, t]);

  const send = async () => {
    if (!canSend) return;
    const text = value;
    setValue('');
    await chatPeer.sendText(friend.id, text);
  };

  const download = async (message: ChatMessage) => {
    if (!message.file) return;
    if (message.file.storedName) await window.windowControls.chatFileSaveAs(message.file.storedName, message.file.name);
    else await chatPeer.requestFile(message);
  };

  const handleDragEnter = (event: React.DragEvent<HTMLDivElement>) => {
    event.preventDefault();
    dragDepth.current += 1;
    if (event.dataTransfer.types.includes('Files')) setDragging(true);
  };

  const handleDragLeave = (event: React.DragEvent<HTMLDivElement>) => {
    event.preventDefault();
    dragDepth.current -= 1;
    if (dragDepth.current <= 0) {
      dragDepth.current = 0;
      setDragging(false);
    }
  };

  const handleDrop = (event: React.DragEvent<HTMLDivElement>) => {
    event.preventDefault();
    dragDepth.current = 0;
    setDragging(false);
    const files = Array.from(event.dataTransfer.files).filter(file => file.size <= 2 * 1024 * 1024 * 1024);
    if (files.length > 0) setDroppedFiles(files);
  };

  const confirmDroppedFiles = async () => {
    const paths = droppedFiles.map(file => window.windowControls.getPathForFile(file)).filter((path): path is string => Boolean(path));
    setDroppedFiles([]);
    await chatPeer.sendDroppedFiles(friend.id, paths);
  };

  return (
    <div className="absolute inset-3 z-[80] bg-panelBg border border-white/[0.07] border-t-white/[0.14] rounded-panel flex flex-col overflow-hidden animate-fade-in" onDragEnter={handleDragEnter} onDragOver={event => event.preventDefault()} onDragLeave={handleDragLeave} onDrop={handleDrop}>
      <div className="h-16 px-4 grid grid-cols-[40px_minmax(0,1fr)_40px] items-center gap-3 border-b border-white/[0.07] shrink-0">
        <div className="group relative flex items-center justify-center" aria-label={t('chat.information')}>
          <Info weight="bold" size={18} className="text-white/30 transition-colors group-hover:text-white/70" />
          <div role="tooltip" className="pointer-events-none absolute left-0 top-[calc(100%+12px)] z-[100] w-64 rounded-[14px] border border-white/[0.07] border-t-white/[0.14] bg-panelBg/95 p-3 text-left opacity-0 backdrop-blur-xl transition-opacity group-hover:opacity-100">
            <div className="text-sm font-medium text-white">{t('chat.retentionHint')}</div>
            <div className="mt-2 text-xs text-textMuted">{t('chat.encryptionHint')}</div>
          </div>
        </div>
        <div className="min-w-0 text-center">
          <div className="font-bold text-white truncate">{friend.displayName}</div>
          <div className="text-xs text-textMuted truncate">{status}</div>
        </div>
        <button onClick={onCall} disabled={!friend.isOnline} className="w-10 h-10 rounded-full bg-surface/70 hover:bg-surfaceHover/80 disabled:opacity-40 text-white flex items-center justify-center transition-transform active:scale-95" title={t('chat.call')}>
          <PhoneCall weight="bold" size={19} />
        </button>
      </div>

      <div className="chat-scroll flex-1 min-h-0 overflow-y-auto p-4">
        {messages.length === 0 ? (
          <div className="h-full flex items-center justify-center text-textMuted font-medium">{t('chat.empty')}</div>
        ) : (
          <div className="flex flex-col gap-2">
            {messages.map(message => {
              const own = message.senderId === currentUser.id;
              return (
                <div key={message.id} className={`flex ${own ? 'justify-end' : 'justify-start'}`}>
                  <div
                    onContextMenu={event => { event.preventDefault(); event.stopPropagation(); setMenu({ x: Math.min(event.clientX, window.innerWidth - 208), y: Math.min(event.clientY, window.innerHeight - 180), message }); }}
                    className={`max-w-[72%] rounded-[14px] px-4 py-3 border ${own ? 'bg-primary/20 border-primary/30' : 'bg-surface/70 border-white/[0.07]'}`}
                  >
                    {message.kind === 'text' ? (
                      <p className="text-white text-sm whitespace-pre-wrap break-words select-text">{message.text}</p>
                    ) : message.file && (
                      <div className="flex items-center gap-3 min-w-[240px]">
                        <div className="w-10 h-10 rounded-xl bg-white/[0.06] flex items-center justify-center shrink-0"><File weight="bold" size={20} /></div>
                        <div className="min-w-0 flex-1">
                          <div className="text-white text-sm font-semibold truncate">{message.file.name}</div>
                          <div className="text-textMuted text-xs">{formatSize(message.file.size, t)}</div>
                          {message.file.transferState === 'transferring' && <div className="h-1 bg-black/30 rounded-full overflow-hidden mt-2"><div className="h-full bg-primary origin-left" style={{ transform: `scaleX(${message.file.progress})` }} /></div>}
                        </div>
                        <button onClick={() => void download(message)} disabled={message.file.transferState === 'transferring'} className="w-9 h-9 rounded-xl bg-surface/70 hover:bg-surfaceHover/80 disabled:opacity-40 flex items-center justify-center active:scale-95" title={message.file.storedName ? t('chat.saveFile') : t('chat.download')}>
                          <DownloadSimple weight="bold" size={17} />
                        </button>
                      </div>
                    )}
                    <div className="mt-1.5 text-[10px] text-white/35 text-right">
                      {new Date(message.createdAt).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}{own ? ` · ${t(`chat.delivery.${message.delivery}`)}` : ''}
                    </div>
                  </div>
                </div>
              );
            })}
            <div ref={bottomRef} />
          </div>
        )}
      </div>

      <div className="p-4 border-t border-white/[0.07] flex items-end gap-2 shrink-0">
        <button onClick={() => void chatPeer.sendFiles(friend.id)} className="w-11 h-11 rounded-[14px] bg-surface/70 hover:bg-surfaceHover/80 text-white flex items-center justify-center transition-transform active:scale-95" title={t('chat.attach')}>
          <Paperclip weight="bold" size={20} />
        </button>
        <textarea value={value} onChange={event => setValue(event.target.value.slice(0, 4000))} onKeyDown={event => { if (event.key === 'Enter' && !event.shiftKey) { event.preventDefault(); void send(); } }} rows={1} placeholder={t('chat.placeholder')} className="glass-field flex-1 resize-none min-h-11 max-h-32 px-4 py-3 text-sm rounded-[14px]" />
        <button onClick={() => void send()} disabled={!canSend} className="w-11 h-11 rounded-[14px] bg-primary/90 hover:opacity-90 disabled:opacity-40 text-white flex items-center justify-center transition-transform active:scale-95">
          <PaperPlaneRight weight="bold" size={20} />
        </button>
      </div>

      {menu && createPortal(
        <div ref={menuRef} className="fixed z-[999999] glass-sheet py-2 w-48 rounded-[14px]" style={{ left: menu.x, top: menu.y }} onClick={event => event.stopPropagation()} onContextMenu={event => { event.preventDefault(); event.stopPropagation(); }}>
          {menu.message.kind === 'text' && <button onClick={() => { void navigator.clipboard.writeText(menu.message.text); setMenu(null); }} className="w-full text-left px-4 py-2 flex items-center gap-3 text-white hover:bg-surfaceHover/80"><Copy weight="bold" size={16} />{t('chat.copy')}</button>}
          {menu.message.file?.storedName && <button onClick={() => { void chatPeer.clearFile(menu.message); setMenu(null); }} className="w-full text-left px-4 py-2 flex items-center gap-3 text-white hover:bg-surfaceHover/80"><Broom weight="bold" size={16} />{t('chat.clearFile')}</button>}
          <button onClick={() => { void chatPeer.deleteLocal(menu.message); setMenu(null); }} className="w-full text-left px-4 py-2 flex items-center gap-3 text-danger hover:bg-surfaceHover/80"><Trash weight="bold" size={16} />{t('chat.deleteMessage')}</button>
        </div>,
        document.body
      )}
      {dragging && (
        <div className="absolute inset-0 z-[99999] bg-black/70 backdrop-blur-md flex items-center justify-center pointer-events-none">
          <div className="glass-modal w-[400px] p-8 text-center">
            <PaperPlaneRight weight="bold" size={40} className="mx-auto mb-4 text-primaryText" />
            <div className="text-xl font-bold text-white">{t('chat.dropToSend')}</div>
          </div>
        </div>
      )}
      {droppedFiles.length > 0 && createPortal(
        <div className="fixed inset-0 z-[100000] bg-black/70 backdrop-blur-md flex items-center justify-center p-3 pt-[3.75rem]">
          <div className="glass-modal w-[400px] p-6">
            <div className="flex items-center justify-between mb-4">
              <h2 className="text-xl font-bold text-white">{t('chat.sendFileConfirm')}</h2>
              <button onClick={() => setDroppedFiles([])} className="p-1.5 rounded-lg text-textMuted hover:text-white hover:bg-surface/70 transition-colors"><X weight="bold" size={24} /></button>
            </div>
            <div className="max-h-48 overflow-y-auto space-y-2 mb-6">
              {droppedFiles.map((file, index) => <div key={`${file.name}-${index}`} className="glass-field rounded-xl p-3 text-sm text-white truncate">{file.name}</div>)}
            </div>
            <div className="grid grid-cols-2 gap-2">
              <button onClick={() => setDroppedFiles([])} className="py-3 rounded-xl font-bold bg-surface/70 text-white hover:bg-surfaceHover/80 active:scale-[0.98] transition-all">{t('common.cancel')}</button>
              <button onClick={() => void confirmDroppedFiles()} className="py-3 rounded-xl font-bold bg-primary/90 text-white hover:opacity-90 active:scale-[0.98] transition-all">{t('chat.sendFile')}</button>
            </div>
          </div>
        </div>,
        document.body
      )}
    </div>
  );
}
