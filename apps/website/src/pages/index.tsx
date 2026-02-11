import type {ReactNode} from 'react';
import Link from '@docusaurus/Link';
import useDocusaurusContext from '@docusaurus/useDocusaurusContext';
import Layout from '@theme/Layout';
import Heading from '@theme/Heading';

import styles from './index.module.css';

const deploymentCards = [
  {
    title: 'Open Source Core',
    body: 'Build and self-host under AGPL-3.0. Full protocol and runtime control for teams that want maximum flexibility.',
    to: '/docs/deployment-modes#open-source-self-hosted-agpl-30',
    cta: 'Self-host with OSS',
  },
  {
    title: 'Commercial Licensing',
    body: 'Use private modifications, embed Wavry into closed products, or ship internal-only deployments with commercial terms.',
    to: '/docs/deployment-modes#commercial-license',
    cta: 'Review commercial path',
  },
  {
    title: 'Hosted Control Plane',
    body: 'Use official auth, matchmaking, and relay services for faster onboarding while keeping end-to-end encrypted sessions.',
    to: '/docs/deployment-modes#official-hosted-services',
    cta: 'See hosted service model',
  },
];

const docTracks = [
  {
    title: 'Get Running',
    body: 'Install dependencies, run the local stack, and launch desktop clients.',
    to: '/docs/getting-started',
  },
  {
    title: 'Design and Integrate',
    body: 'Understand components, protocol boundaries, and transport decisions.',
    to: '/docs/architecture',
  },
  {
    title: 'Secure and Operate',
    body: 'Review encryption, threat model assumptions, and production operations.',
    to: '/docs/security',
  },
  {
    title: 'Ship Releases',
    body: 'Use CI/CD packaging, desktop distribution guidance, and release checklists.',
    to: '/docs/operations',
  },
];

function HomepageHeader() {
  const {siteConfig} = useDocusaurusContext();

  return (
    <header className={styles.hero}>
      <div className={styles.heroShell}>
        <p className={styles.eyebrow}>wavry.dev</p>
        <Heading as="h1" className={styles.heroTitle}>
          Remote desktop infrastructure designed for responsive, low-latency control.
        </Heading>
        <p className={styles.heroSubtitle}>{siteConfig.tagline}</p>
        <div className={styles.heroActions}>
          <Link className="button button--primary button--lg" to="/docs/getting-started">
            Get Started
          </Link>
          <Link className="button button--outline button--lg" to="/docs/deployment-modes">
            OSS, Commercial, Hosted
          </Link>
        </div>
      </div>
    </header>
  );
}

export default function Home(): ReactNode {
  return (
    <Layout
      title="Wavry"
      description="Public-facing Wavry documentation for open source, commercial licensing, hosted usage, and technical operations.">
      <HomepageHeader />
      <main>
        <section className={styles.section}>
          <div className={styles.sectionInner}>
            <div className={styles.sectionHeader}>
              <Heading as="h2" className={styles.sectionTitle}>
                Choose the deployment model that fits your team
              </Heading>
              <p className={styles.sectionText}>
                Wavry is intentionally split into open source and commercial pathways, with hosted infrastructure available when
                you need faster rollout.
              </p>
            </div>
            <div className={styles.cardGrid}>
              {deploymentCards.map((card) => (
                <article key={card.title} className={styles.card}>
                  <Heading as="h3" className={styles.cardTitle}>
                    {card.title}
                  </Heading>
                  <p className={styles.cardBody}>{card.body}</p>
                  <Link className={styles.cardLink} to={card.to}>
                    {card.cta}
                  </Link>
                </article>
              ))}
            </div>
          </div>
        </section>

        <section className={styles.sectionAlt}>
          <div className={styles.sectionInner}>
            <div className={styles.sectionHeader}>
              <Heading as="h2" className={styles.sectionTitle}>
                Documentation tracks
              </Heading>
              <p className={styles.sectionText}>
                Start with overview docs, then go deeper into protocol, security, and deployment operations.
              </p>
            </div>
            <div className={styles.trackGrid}>
              {docTracks.map((track) => (
                <Link className={styles.trackCard} to={track.to} key={track.title}>
                  <Heading as="h3" className={styles.trackTitle}>
                    {track.title}
                  </Heading>
                  <p className={styles.trackBody}>{track.body}</p>
                </Link>
              ))}
            </div>
          </div>
        </section>
      </main>
    </Layout>
  );
}
