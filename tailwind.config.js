
module.exports = {
  content: [
    "./src/**/*.{js,jsx,ts,tsx,html}",
  ],
  theme: {
    extend: {
      colors: {
        appBg: '#0B0B0B',
        panelBg: '#161618',
        surface: '#222225',
        surfaceHover: '#2A2A2E',
        accent: '#5865F2',
        danger: '#DA373C',
        success: '#23A559',
        warning: '#FBBF24',
        textMain: '#F2F3F5',
        textMuted: '#949BA4'
      },
      borderRadius: {
        'md': '0.5rem',
        'lg': '0.75rem',
        'xl': '1rem',
        '2xl': '1.5rem',
        '3xl': '2rem',
      }
    },
  },
  plugins: [],
}