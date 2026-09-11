import { useEffect, useRef, useState } from 'react';
import { PhoneCall } from '@phosphor-icons/react';
import { useTranslation } from 'react-i18next';
import type { User } from '../../store/useAppStore';
import { AvatarImg } from '../Shared/AvatarImg';

type Position = { x: number; y: number };

export function ActiveSessionPip({ callUser, onOpen }: { callUser: User; onOpen: () => void }) {
  const { t } = useTranslation();
  const buttonRef = useRef<HTMLButtonElement>(null);
  const positionRef = useRef<Position>({ x: 0, y: 0 });
  const pointerRef = useRef<{ id: number; x: number; y: number; originX: number; originY: number; moved: boolean } | null>(null);
  const [position, setPosition] = useState<Position>({ x: 0, y: 0 });
  const [dragging, setDragging] = useState(false);

  useEffect(() => {
    positionRef.current = position;
  }, [position]);

  useEffect(() => {
    const constrain = () => {
      const element = buttonRef.current;
      if (!element) return;
      const rect = element.getBoundingClientRect();
      const margin = 12;
      const next = {
        x: Math.min(Math.max(positionRef.current.x, margin - rect.left + positionRef.current.x), window.innerWidth - margin - rect.right + positionRef.current.x),
        y: Math.min(Math.max(positionRef.current.y, margin - rect.top + positionRef.current.y), window.innerHeight - margin - rect.bottom + positionRef.current.y)
      };
      positionRef.current = next;
      setPosition(next);
    };
    window.addEventListener('resize', constrain);
    return () => window.removeEventListener('resize', constrain);
  }, []);

  const constrainPosition = (x: number, y: number) => {
    const element = buttonRef.current;
    if (!element) return { x, y };
    const rect = element.getBoundingClientRect();
    const baseLeft = rect.left - positionRef.current.x;
    const baseTop = rect.top - positionRef.current.y;
    const margin = 12;
    return {
      x: Math.min(Math.max(x, margin - baseLeft), window.innerWidth - margin - rect.width - baseLeft),
      y: Math.min(Math.max(y, margin - baseTop), window.innerHeight - margin - rect.height - baseTop)
    };
  };

  return (
    <button
      ref={buttonRef}
      type="button"
      onPointerDown={event => {
        if (event.button !== 0) return;
        pointerRef.current = { id: event.pointerId, x: event.clientX, y: event.clientY, originX: positionRef.current.x, originY: positionRef.current.y, moved: false };
        event.currentTarget.setPointerCapture(event.pointerId);
      }}
      onPointerMove={event => {
        const pointer = pointerRef.current;
        if (!pointer || pointer.id !== event.pointerId) return;
        const dx = event.clientX - pointer.x;
        const dy = event.clientY - pointer.y;
        if (!pointer.moved && Math.hypot(dx, dy) < 4) return;
        pointer.moved = true;
        setDragging(true);
        const next = constrainPosition(pointer.originX + dx, pointer.originY + dy);
        positionRef.current = next;
        setPosition(next);
      }}
      onPointerUp={event => {
        const pointer = pointerRef.current;
        if (!pointer || pointer.id !== event.pointerId) return;
        pointerRef.current = null;
        setDragging(false);
        if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
        if (!pointer.moved) onOpen();
      }}
      onPointerCancel={event => {
        if (pointerRef.current?.id !== event.pointerId) return;
        pointerRef.current = null;
        setDragging(false);
      }}
      className={`fixed right-6 bottom-6 z-[90] glass-sheet border border-white/[0.07] border-t-white/[0.14] rounded-[14px] p-3 flex items-center gap-3 w-64 text-left select-none touch-none transition-[background-color,opacity] hover:bg-surfaceHover/80 active:scale-[0.98] ${dragging ? 'cursor-grabbing' : 'cursor-grab'}`}
      style={{ transform: `translate3d(${position.x}px, ${position.y}px, 0)`, willChange: dragging ? 'transform' : undefined }}
      title={t('chat.returnToCall')}
    >
      <div className="w-11 h-11 shrink-0"><AvatarImg src={callUser.avatarBase64} size={44} bgColor={callUser.avatarColor} animate={false} /></div>
      <div className="min-w-0 flex-1"><div className="font-bold text-white text-sm truncate">{callUser.displayName}</div><div className="text-xs text-textMuted">{t('chat.activeCall')}</div></div>
      <PhoneCall weight="bold" size={18} className="text-primaryText shrink-0" />
    </button>
  );
}
