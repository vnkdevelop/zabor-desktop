export {}

type EncodedFrame = {
  timestamp: number
  data: ArrayBuffer
  getMetadata?: () => { sequenceNumber?: number }
}

type RtcTransformer = {
  readable: ReadableStream<EncodedFrame>
  writable: WritableStream<EncodedFrame>
  options?: unknown
}

type RtcTransformEvent = {
  transformer: RtcTransformer
}

type TransformWorkerScope = {
  onrtctransform: ((event: RtcTransformEvent) => void) | null
  postMessage: (message: unknown, transfer?: Transferable[]) => void
}

const scope = self as unknown as TransformWorkerScope

const describeError = (error: unknown): string => {
  const message = (error as { message?: string })?.message
  return message ? String(message) : String(error)
}

scope.onrtctransform = (event) => {
  scope.postMessage({ type: 'connected' })
  const reader = event.transformer.readable.getReader()
  let posts = 0
  const pump = (): void => {
    reader.read().then(({ done, value }) => {
      if (done) {
        scope.postMessage({ type: 'ended', reason: 'closed', posts })
        return
      }
      if (!value) {
        pump()
        return
      }
      let sequenceNumber: number | undefined
      try {
        sequenceNumber = value.getMetadata ? value.getMetadata().sequenceNumber : undefined
      } catch {
        sequenceNumber = undefined
      }
      try {
        scope.postMessage({ timestamp: value.timestamp, sequenceNumber, data: value.data }, [value.data])
        posts++
      } catch (error) {
        scope.postMessage({ type: 'ended', reason: 'post:' + describeError(error), posts })
        return
      }
      pump()
    }).catch((error) => {
      scope.postMessage({ type: 'ended', reason: 'read:' + describeError(error), posts })
    })
  }
  pump()
}
