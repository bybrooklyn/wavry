import type {ReactNode} from 'react';
import Link from '@docusaurus/Link';
import useDocusaurusContext from '@docusaurus/useDocusaurusContext';
import Layout from '@theme/Layout';
import Heading from '@theme/Heading';

import styles from './index.module.css';

const deploymentCards = [
  {
    title: 'Open Source Core',
    body: 'Run and modify the full stack under AGPL-3.0 when infra control and transparency are the priority.',
    to: '/docs/deployment-modes#open-source-self-hosted-agpl-30',
    cta: 'Self-host with OSS',
  },
  {
    title: 'Commercial Licensing',
    body: 'Use private forks, proprietary embedding, or closed-source distribution paths under commercial terms.',
    to: '/docs/deployment-modes#commercial-license',
    cta: 'Review commercial path',
  },
  {
    title: 'Hosted Control Plane',
    body: 'Use managed signaling, auth, and relay assistance when you need faster operational rollout.',
    to: '/docs/deployment-modes#official-hosted-services',
    cta: 'See hosted model',
  },
];

const docTracks = [
  {
    title: 'Understand Wavry',
    body: 'Start with product overview, use cases, and architecture boundaries.',
    to: '/docs/overview',
  },
  {
    title: 'Run It Locally',
    body: 'Bring up gateway, relay, host, and client in a practical first session.',
    to: '/docs/getting-started',
  },
  {
    title: 'Deploy Safely',
    body: 'Review security expectations, controls, and production operations guidance.',
    to: '/docs/security',
  },
  {
    title: 'Operate and Release',
    body: 'Use CI/CD, packaging, and release checks to keep shipping predictable.',
    to: '/docs/operations',
  },
];

const useCases = [
  {
    title: 'Remote Workstations',
    body: 'Interactive desktop control for creative, engineering, and support workloads.',
  },
  {
    title: 'Cloud Gaming Sessions',
    body: 'Low-latency session delivery where delayed input is unacceptable.',
  },
  {
    title: 'Embedded Streaming Products',
    body: 'A Rust-native base for products that need secure remote interaction at scale.',
  },
];

const workflowSteps = [
  {
    title: 'Signal and establish path',
    body: 'Client and host coordinate direct connectivity, with relay only when required.',
  },
  {
    title: 'Negotiate encrypted transport',
    body: 'Peers establish session keys and authenticated encrypted packet exchange.',
  },
  {
    title: 'Stream media and control input',
    body: 'Host sends media while client sends input events over low-latency control paths.',
  },
  {
    title: 'Adapt continuously',
    body: 'DELTA congestion control adjusts bitrate/FEC to keep response time stable.',
  },
];

function HomepageHeader() {
  const {siteConfig} = useDocusaurusContext();

  return (
    <header className={styles.hero}>
      <div className={styles.heroShell}>
        <p className={styles.eyebrow}>Wavry Platform</p>
        <Heading as="h1" className={styles.heroTitle}>
          End-to-end encrypted remote sessions built for low-latency control.
        </Heading>
        <p className={styles.heroSubtitle}>
          {siteConfig.tagline} Wavry is for teams shipping remote desktop and interactive streaming products where
          responsiveness matters as much as reliability.
        </p>
        <div className={styles.heroActions}>
          <Link className="button button--primary button--lg" to="/docs/overview">
            What is Wavry?
          </Link>
          <Link className="button button--outline button--lg" to="/docs/getting-started">
            Run the stack locally
          </Link>
        </div>
        <ul className={styles.heroFacts}>
          <li>P2P-first transport</li>
          <li>Mandatory encrypted sessions</li>
          <li>Rust protocol and runtime core</li>
        </ul>
      </div>
    </header>
  );
}

export default function Home(): ReactNode {
  return (
    <Layout
      title="Wavry"
      description="Wavry documentation for low-latency remote desktop, cloud streaming, deployment modes, and operations.">
      <HomepageHeader />
      <main>
        <section className={styles.section}>
          <div className={styles.sectionInner}>
            <div className={styles.sectionHeader}>
              <Heading as="h2" className={styles.sectionTitle}>
                What this software is for
              </Heading>
              <p className={styles.sectionText}>
                Wavry is not a generic video streaming stack. It is purpose-built for interactive sessions that require
                fast round-trip input handling and stable latency under changing network conditions.
              </p>
            </div>
            <div className={styles.cardGrid}>
              {useCases.map((card) => (
                <article key={card.title} className={styles.card}>
                  <Heading as="h3" className={styles.cardTitle}>
                    {card.title}
                  </Heading>
                  <p className={styles.cardBody}>{card.body}</p>
                </article>
              ))}
            </div>
          </div>
        </section>

        <section className={styles.sectionAlt}>
          <div className={styles.sectionInner}>
            <div className={styles.sectionHeader}>
              <Heading as="h2" className={styles.sectionTitle}>
                How a Wavry session works
              </Heading>
              <p className={styles.sectionText}>The platform keeps control-plane coordination separate from encrypted media flow.</p>
            </div>
            <ol className={styles.stepList}>
              {workflowSteps.map((step) => (
                <li key={step.title} className={styles.stepItem}>
                  <Heading as="h3" className={styles.stepTitle}>
                    {step.title}
                  </Heading>
                  <p className={styles.stepBody}>{step.body}</p>
                </li>
              ))}
            </ol>
          </div>
        </section>

        <section className={styles.section}>
          <div className={styles.sectionInner}>
            <div className={styles.sectionHeader}>
              <Heading as="h2" className={styles.sectionTitle}>
                Pick your deployment model
              </Heading>
              <p className={styles.sectionText}>
                Choose open source, commercial licensing, or hosted control plane based on compliance, speed, and product needs.
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
              <p className={styles.sectionText}>Use these paths to evaluate, deploy, and operate Wavry with fewer unknowns.</p>
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
