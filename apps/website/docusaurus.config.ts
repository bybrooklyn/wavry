import {themes as prismThemes} from 'prism-react-renderer';
import type {Config} from '@docusaurus/types';
import type * as Preset from '@docusaurus/preset-classic';

const config: Config = {
  title: 'Wavry',
  tagline: 'Latency-first remote desktop, game streaming, and cloud delivery.',
  favicon: 'img/favicon.ico',

  future: {
    v4: true,
  },

  url: 'https://wavry.dev',
  baseUrl: '/',

  organizationName: 'bybrooklyn',
  projectName: 'wavry',

  onBrokenLinks: 'throw',
  markdown: {
    hooks: {
      onBrokenMarkdownLinks: 'warn',
    },
  },

  i18n: {
    defaultLocale: 'en',
    locales: ['en'],
  },

  presets: [
    [
      'classic',
      {
        docs: {
          routeBasePath: 'docs',
          sidebarPath: './sidebars.ts',
          editUrl: 'https://github.com/bybrooklyn/wavry/edit/main/apps/website/',
        },
        blog: false,
        theme: {
          customCss: './src/css/custom.css',
        },
      } satisfies Preset.Options,
    ],
  ],

  themeConfig: {
    image: 'img/logo.png',
    colorMode: {
      defaultMode: 'light',
      respectPrefersColorScheme: true,
    },
    navbar: {
      title: 'Wavry',
      logo: {
        alt: 'Wavry Logo',
        src: 'img/logo.png',
      },
      items: [
        {
          type: 'docSidebar',
          sidebarId: 'docsSidebar',
          position: 'left',
          label: 'Documentation',
        },
        {
          type: 'doc',
          docId: 'deployment-modes',
          position: 'left',
          label: 'OSS + Commercial',
        },
        {
          href: 'https://github.com/bybrooklyn/wavry',
          label: 'GitHub',
          position: 'right',
        },
      ],
    },
    footer: {
      style: 'dark',
      links: [
        {
          title: 'Start Here',
          items: [
            {
              label: 'Overview',
              to: '/docs/overview',
            },
            {
              label: 'Getting Started',
              to: '/docs/getting-started',
            },
            {
              label: 'Deployment Modes',
              to: '/docs/deployment-modes',
            },
          ],
        },
        {
          title: 'Technical',
          items: [
            {
              label: 'Architecture',
              to: '/docs/architecture',
            },
            {
              label: 'Security',
              to: '/docs/security',
            },
            {
              label: 'Operations',
              to: '/docs/operations',
            },
          ],
        },
        {
          title: 'More',
          items: [
            {
              label: 'Commercial Terms',
              href: 'https://github.com/bybrooklyn/wavry/blob/main/COMMERCIAL.md',
            },
            {
              label: 'Hosted Terms',
              href: 'https://github.com/bybrooklyn/wavry/blob/main/TERMS.md',
            },
            {
              label: 'License',
              href: 'https://github.com/bybrooklyn/wavry/blob/main/LICENSE',
            },
          ],
        },
      ],
      copyright: `Copyright © ${new Date().getFullYear()} Wavry.`,
    },
    prism: {
      theme: prismThemes.github,
      darkTheme: prismThemes.dracula,
    },
  } satisfies Preset.ThemeConfig,
};

export default config;
