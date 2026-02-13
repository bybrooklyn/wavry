import {themes as prismThemes} from 'prism-react-renderer';
import type {Config} from '@docusaurus/types';
import type * as Preset from '@docusaurus/preset-classic';

const config: Config = {
  title: 'Wavry',
  tagline: 'Latency-first remote sessions for desktop control, game streaming, and cloud apps.',
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
          routeBasePath: '/',
          sidebarPath: './sidebars.ts',
          editUrl: 'https://github.com/bybrooklyn/wavry/edit/main/apps/website/',
        },
        pages: false,
        blog: false,
        theme: {
          customCss: './src/css/custom.css',
        },
      } satisfies Preset.Options,
    ],
  ],

  themeConfig: {
    image: 'img/logo.png',
    docs: {
      sidebar: {
        hideable: false,
        autoCollapseCategories: false,
      },
    },
    colorMode: {
      defaultMode: 'dark',
      disableSwitch: true,
      respectPrefersColorScheme: false,
    },
    navbar: {
      title: 'Wavry',
      logo: {
        alt: 'Wavry Logo',
        src: 'img/logo.png',
      },
      items: [
        {
          type: 'doc',
          docId: 'overview',
          position: 'left',
          label: 'Overview',
        },
        {
          type: 'doc',
          docId: 'getting-started',
          position: 'left',
          label: 'Getting Started',
        },
        {
          type: 'doc',
          docId: 'documentation-map',
          position: 'left',
          label: 'Docs Map',
        },
        {
          type: 'doc',
          docId: 'codebase-reference',
          position: 'left',
          label: 'Reference',
        },
        {
          type: 'doc',
          docId: 'linux-wayland-support',
          position: 'left',
          label: 'Linux + Wayland',
        },
        {
          to: '/pricing',
          label: 'Pricing',
          position: 'right',
          className: 'navbar-pricing-button',
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
              to: '/',
            },
            {
              label: 'Getting Started',
              to: '/getting-started',
            },
            {
              label: 'Documentation Map',
              to: '/documentation-map',
            },
            {
              label: 'Codebase Reference',
              to: '/codebase-reference',
            },
            {
              label: 'Deployment Modes',
              to: '/deployment-modes',
            },
            {
              label: 'Pricing',
              to: '/pricing',
            },
          ],
        },
        {
          title: 'Technical',
          items: [
            {
              label: 'Architecture',
              to: '/architecture',
            },
            {
              label: 'Lifecycle',
              to: '/lifecycle',
            },
            {
              label: 'Networking',
              to: '/networking-and-relay',
            },
            {
              label: 'Security',
              to: '/security',
            },
            {
              label: 'Env Vars',
              to: '/environment-variable-reference',
            },
            {
              label: 'Operations',
              to: '/operations',
            },
          ],
        },
        {
          title: 'Licensing',
          items: [
            {
              label: 'RIFT / AGPL-3.0 License',
              href: 'https://github.com/bybrooklyn/wavry/blob/main/LICENSE',
            },
            {
              label: 'Commercial Terms',
              href: 'https://github.com/bybrooklyn/wavry/blob/main/COMMERCIAL.md',
            },
            {
              label: 'Hosted Terms',
              href: 'https://github.com/bybrooklyn/wavry/blob/main/TERMS.md',
            },
            {
              label: 'contact@wavry.dev',
              href: 'mailto:contact@wavry.dev',
            },
          ],
        },
      ],
      copyright: `Copyright © ${new Date().getFullYear()} Wavry.`,
    },
    prism: {
      theme: prismThemes.dracula,
      darkTheme: prismThemes.dracula,
    },
  } satisfies Preset.ThemeConfig,
};

export default config;
