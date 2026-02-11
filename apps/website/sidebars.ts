import type {SidebarsConfig} from '@docusaurus/plugin-content-docs';

const sidebars: SidebarsConfig = {
  docsSidebar: [
    'overview',
    'getting-started',
    {
      type: 'category',
      label: 'Product and Deployment',
      items: ['deployment-modes', 'desktop-app', 'roadmap'],
    },
    {
      type: 'category',
      label: 'Technical Foundation',
      items: ['architecture', 'security', 'operations'],
    },
    'faq',
  ],
};

export default sidebars;
