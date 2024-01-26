import colors from 'tailwindcss/colors'
import plugin from 'tailwindcss/plugin'
import type { Config } from 'tailwindcss'
import tailwindAnimate from 'tailwindcss-animate'
import defaultTheme from 'tailwindcss/defaultTheme'
import typographyPlugin from '@tailwindcss/typography'
import starlightPlugin from '@astrojs/starlight-tailwind'
import aspectRatioPlugin from '@tailwindcss/aspect-ratio'
import containerQueriesPlugin from '@tailwindcss/container-queries'

export default ({
  content: ['./src/**/*.{astro,html,js,jsx,md,mdx,svelte,ts,tsx,vue}'],
  darkMode: 'class',
  important: true,
  future: { hoverOnlyWhenSupported: true },
  theme: {
    transparent: 'transparent',
    current: 'currentColor',
    extend: {
      screens: {
        xs: '320px'
      },
      colors: {
        accent: {
          DEFAULT: '#A0ECFD',
          50: '#FAFEFF',
          100: '#F0FCFF',
          200: '#DCF8FE',
          300: '#C8F4FE',
          400: '#B4F0FD',
          500: '#A0ECFD',
          600: '#5FDFFC',
          700: '#1ED2FA',
          800: '#04ACD2',
          900: '#037791',
          950: '#025C70'
        }
      },
      fontFamily: {
        mono: ['JetBrainsMono', ...defaultTheme.fontFamily.mono],
        sans: [
          '"Inter var", sans-serif',
          {
            fontFeatureSettings: '"cv11", "ss01"',
            fontVariationSettings: '"opsz" 32'
          }
        ],
        serif: ['"Inter var"'],
        argon: ['"Monospace Argon"']
      }
    }
  },
  plugins: [
    starlightPlugin(),
    tailwindAnimate,
    typographyPlugin,
    aspectRatioPlugin,
    containerQueriesPlugin,
    plugin(({ addVariant, addUtilities, matchUtilities, theme }) => {
      matchUtilities(
        { 'animation-delay': value => ({ 'animation-delay': value }) },
        { values: theme('transitionDelay') }
      )
      addVariant('optional', '&:optional')
      addVariant('hocus', ['&:hover', '&:focus'])
      addVariant('inverted-colors', '@media (inverted-colors: inverted)')
      addUtilities({
        '.content-auto': { 'content-visibility': 'auto' },
        '.content-hidden': { 'content-visibility': 'hidden' },
        '.content-visible': { 'content-visibility': 'visible' }
      })
    })
  ]
} satisfies Config)
