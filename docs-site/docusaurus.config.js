// User-facing Markdown is maintained once, in docs/user.
const baseUrl = process.env.DOCS_BASE_URL || '/';
if (!baseUrl.startsWith('/') || !baseUrl.endsWith('/')) {
  throw new Error('DOCS_BASE_URL must start and end with a slash.');
}

export default {
  title: 'Yoto Atelier',
  tagline: 'Your audio library, your stories, your playlists.',
  url: process.env.DOCS_URL || 'http://localhost:3000',
  baseUrl,
  trailingSlash: true,
  favicon: 'img/app-icon.ico',
  onBrokenLinks: 'throw',
  i18n: { defaultLocale: 'en', locales: ['en'] },
  markdown: { format: 'detect', hooks: { onBrokenMarkdownLinks: 'throw' } },
  presets: [['classic', {
    docs: {
      path: '../docs/user',
      routeBasePath: '/',
      sidebarPath: './sidebars.js',
    },
    blog: false,
    theme: { customCss: './src/css/custom.css' },
  }]],
  themeConfig: {
    colorMode: { respectPrefersColorScheme: true },
    navbar: {
      title: 'Yoto Atelier',
      logo: { alt: 'Yoto Atelier audio library', src: 'img/portal-logo.png' },
      items: [{ type: 'docSidebar', sidebarId: 'guide', label: 'User guide', position: 'left' }],
    },
    footer: {
      style: 'dark',
      links: [{ title: 'Getting started', items: [
        { label: 'Your first playlist', to: '/getting-started/' },
        { label: 'Troubleshooting', to: '/troubleshooting/' },
        { label: 'Back up your library', to: '/backups/' },
      ] }],
      copyright: 'Yoto Atelier · Your personal audio library',
    },
  },
};
