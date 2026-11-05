import 'react'

declare module 'react' {
  interface CSSProperties {
    WebkitAppRegion?: 'drag' | 'no-drag'
  }
}

declare global {
  interface RTCRtpTransceiver {
    readonly stopped?: boolean
  }
}
