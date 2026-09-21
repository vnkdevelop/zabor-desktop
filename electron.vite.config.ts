import { resolve } from 'path'
import { defineConfig, externalizeDepsPlugin } from 'electron-vite'
import { loadEnv } from 'vite'
import react from '@vitejs/plugin-react'

const fileEnv = loadEnv(process.env.NODE_ENV === 'development' ? 'development' : 'production', process.cwd(), '')
const clientSecret = process.env.ZABOR_CLIENT_SECRET || fileEnv.ZABOR_CLIENT_SECRET || ''
const clientChannel =
  process.env.ZABOR_CLIENT_CHANNEL || fileEnv.ZABOR_CLIENT_CHANNEL || (clientSecret ? 'official' : 'unofficial')

function relaxCspForDevServer() {
  return {
    name: 'zabor-relax-csp-for-dev-server',
    apply: 'serve' as const,
    transformIndexHtml(html: string) {
      return html.replace("script-src 'self' 'unsafe-eval'", "script-src 'self' 'unsafe-eval' 'unsafe-inline'")
    }
  }
}

export default defineConfig({
  main: {
    plugins: [externalizeDepsPlugin()],
    define: {
      __ZABOR_CLIENT_SECRET__: JSON.stringify(clientSecret),
      __ZABOR_CLIENT_CHANNEL__: JSON.stringify(clientChannel),
      __ZABOR_UPDATE_SIGNER__: JSON.stringify(
        process.env.ZABOR_UPDATE_SIGNER || fileEnv.ZABOR_UPDATE_SIGNER || ''
      )
    }
  },
  preload: {
    plugins: [externalizeDepsPlugin()]
  },
  renderer: {
    resolve: {
      alias: {
        '@renderer': resolve('src/renderer/src'),
        'deepfilter-wasm-bindgen': resolve('node_modules/deepfilter-standalone/dist/df3/df.js'),
        'rnnoise-wasm-polyfills': resolve('node_modules/@timephy/rnnoise-wasm/dist/polyfills.js'),
        'rnnoise-wasm-processor': resolve('node_modules/@timephy/rnnoise-wasm/dist/RnnoiseProcessor.js'),
        'rnnoise-wasm-sync': resolve('node_modules/@timephy/rnnoise-wasm/dist/generated/rnnoise-sync.js')
      }
    },
    plugins: [react(), relaxCspForDevServer()],
    server: {
      host: '127.0.0.1',
      port: 5173,
      strictPort: true
    }
  }
})
