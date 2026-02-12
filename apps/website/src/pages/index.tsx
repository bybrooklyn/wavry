import type {ReactNode} from 'react';
import Link from '@docusaurus/Link';
import useDocusaurusContext from '@docusaurus/useDocusaurusContext';
import Layout from '@theme/Layout';
import Heading from '@theme/Heading';

import styles from './index.module.css';

const coreHighlights = [
  {
    title: 'Built for Interaction',
    body: 'Wavry is designed for sessions where input timing matters more than static image quality.',
  },
  {
    title: 'Encrypted by Default',
    body: 'Session transport is end-to-end encrypted with replay protection and secure key negotiation.',
  },
  {
    title: 'Rust Protocol Core',
    body: 'RIFT + runtime components are implemented in Rust for predictable performance and maintainability.',
  },
];

const docTracks = [
  {
    title: 'Overview',
    body: 'Understand what Wavry is and where it fits.',
    to: '/docs/overview',
  },
  {
    title: 'Getting Started',
    body: 'Run gateway, relay, host, and client locally.',
    to: '/docs/getting-started',
  },
  {
    title: 'Architecture',
    body: 'See protocol, control-plane, and runtime boundaries.',
    to: '/docs/architecture',
  },
  {
    title: 'Operations',
    body: 'Deploy, monitor, and troubleshoot production setups.',
    to: '/docs/operations',
  },
];

function HomepageHeader() {
  const {siteConfig} = useDocusaurusContext();

  return (
    <header className={styles.hero}>
      <div className={styles.heroShell}>
        <p className={styles.eyebrow}>Wavry</p>
        <Heading as="h1" className={styles.heroTitle}>
          Low-latency remote sessions, built to feel responsive.
        </Heading>
        <p className={styles.heroSubtitle}>
          {siteConfig.tagline} Wavry is for remote desktop and interactive streaming workloads where delayed input
          breaks the experience.
        </p>
        <div className={styles.heroActions}>
          <Link className="button button--primary button--lg" to="/docs/overview">
            Read Overview
          </Link>
          <Link className="button button--outline button--lg" to="/docs/getting-started">
            Start Locally
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
      description="Wavry documentation for low-latency remote desktop and interactive streaming infrastructure.">
      <HomepageHeader />
      <main>
        <section className={styles.section}>
          <div className={styles.sectionInner}>
            <div className={styles.sectionHeader}>
              <Heading as="h2" className={styles.sectionTitle}>
                What you get
              </Heading>
              <p className={styles.sectionText}>
                A practical stack for interactive remote sessions: protocol, crypto, host/client runtimes, and
                deployment guidance.
              </p>
            </div>
            <div className={styles.cardGrid}>
              {coreHighlights.map((item) => (
                <article key={item.title} className={styles.card}>
                  <Heading as="h3" className={styles.cardTitle}>
                    {item.title}
                  </Heading>
                  <p className={styles.cardBody}>{item.body}</p>
                </article>
              ))}
            </div>
          </div>
        </section>

        <section className={styles.sectionAlt}>
          <div className={styles.sectionInner}>
            <div className={styles.licenseCard}>
              <p className={styles.licenseEyebrow}>RIFT License</p>
              <Heading as="h2" className={styles.licenseTitle}>
                RIFT ships under AGPL-3.0 as part of Wavry’s open-source core.
              </Heading>
              <p className={styles.licenseText}>
                If you release source under AGPL terms, you can use Wavry and the RIFT protocol implementation for free.
                If you need exclusion from AGPL obligations and want the Wavry cloud service, use the commercial licensing path.
              </p>
              <div className={styles.licenseActions}>
                <Link className="button button--primary" to="/pricing">
                  View Pricing
                </Link>
                <Link className="button button--outline" to="/docs/deployment-modes">
                  Deployment Modes
                </Link>
                <Link className="button button--outline" href="https://github.com/bybrooklyn/wavry/blob/main/LICENSE">
                  AGPL-3.0 License
                </Link>
              </div>
            </div>
          </div>
        </section>

        <section className={styles.section}>
          <div className={styles.sectionInner}>
            <div className={styles.sectionHeader}>
              <Heading as="h2" className={styles.sectionTitle}>
                Start here
              </Heading>
              <p className={styles.sectionText}>Use these docs to evaluate the software quickly and go deeper where needed.</p>
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
