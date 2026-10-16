import { useEffect, useRef, useState } from 'react'

interface Handlers {
  onEnter?: (el: HTMLElement) => void
  onExit?: (el: HTMLElement) => Promise<void> | void
}

export function usePresence(isOpen: boolean, handlers: Handlers = {}) {
  const [mounted, setMounted] = useState(isOpen)
  const ref = useRef<any>(null)
  const handlersRef = useRef(handlers)
  handlersRef.current = handlers
  const lastAnimatedOpenRef = useRef<boolean | null>(null)

  useEffect(() => {
    let cancelled = false
    if (isOpen) {
      setMounted(true)
    } else if (mounted) {
      const el = ref.current
      if (!el) { setMounted(false); return }
      Promise.resolve(handlersRef.current.onExit?.(el))
        .catch(() => {})
        .then(() => {
          if (!cancelled) setMounted(false)
        })
    }
    return () => { cancelled = true }
  }, [isOpen])

  useEffect(() => {
    if (mounted && isOpen) {
      if (lastAnimatedOpenRef.current !== true) {
        lastAnimatedOpenRef.current = true
        const el = ref.current
        if (el) handlersRef.current.onEnter?.(el)
      }
    } else {
      lastAnimatedOpenRef.current = false
    }
  }, [mounted, isOpen])

  return { mounted, ref }
}
