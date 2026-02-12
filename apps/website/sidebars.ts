import type {SidebarsConfig} from '@docusaurus/plugin-content-docs';

const sidebars: SidebarsConfig = {
  docsSidebar: [
    'overview',
    'getting-started',
    'product-use-cases',
    {
      type: 'category',
      label: 'Build and Integrate',
      items: [
        'architecture',
        'lifecycle',
        'networking-and-relay',
        'configuration-reference',
        'desktop-app',
      ],
    },
    {
      type: 'category',
      label: 'Deploy and Operate',
      items: ['deployment-modes', 'pricing', 'security', 'operations', 'troubleshooting'],
    },
    'roadmap',
    'faq',
  ],
};

export default sidebars;
