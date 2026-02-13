import type {SidebarsConfig} from '@docusaurus/plugin-content-docs';

const sidebars: SidebarsConfig = {
  docsSidebar: [
    'overview',
    'documentation-map',
    'getting-started',
    'installation-and-prerequisites',
    'codebase-reference',
    'runtime-and-service-reference',
    'control-plane-deep-dive',
    'environment-variable-reference',
    'developer-workflows',
    'internal-design-docs',
    'product-use-cases',
    {
      type: 'category',
      label: 'Build and Integrate',
      collapsible: false,
      collapsed: false,
      items: [
        'architecture',
        'lifecycle',
        'networking-and-relay',
        'configuration-reference',
        'desktop-app',
        'linux-wayland-support',
        'linux-production-playbook',
      ],
    },
    {
      type: 'category',
      label: 'Deploy and Operate',
      collapsible: false,
      collapsed: false,
      items: [
        'deployment-modes',
        'docker-control-plane',
        'network-ports-and-firewall',
        'observability-and-alerting',
        'versioning-and-release-policy',
        'upgrade-and-rollback',
        'pricing',
        'security',
        'operations',
        'runbooks-and-checklists',
        'troubleshooting',
        'release-artifacts',
      ],
    },
    'roadmap',
    'faq',
  ],
};

export default sidebars;
