import type {ReactNode} from 'react';
import Layout from '@theme/Layout';
import Heading from '@theme/Heading';
import Link from '@docusaurus/Link';

import styles from './pricing.module.css';

const CONTACT_EMAIL = 'contact@wavry.dev';

const commercialPlans = [
  {
    name: 'Small Team',
    size: '1-5 seats',
    annual: '$70/year flat',
    alternate: 'or $7/month flat',
    formula: 'Flat price covers up to 5 seats total.',
  },
  {
    name: 'Medium Team',
    size: '6-20 seats',
    annual: '$50 base/year + $4 per seat/year',
    alternate: 'Minimum 6 seats, maximum 20 seats',
    formula: 'Yearly total = $50 + ($4 × seat count)',
  },
  {
    name: 'Large Team',
    size: '21+ seats',
    annual: '$100 base/year + $5 per seat/year',
    alternate: 'Minimum 21 seats',
    formula: 'Yearly total = $100 + ($5 × seat count)',
  },
  {
    name: 'SaaS / Integration Tier',
    size: 'Custom agreement',
    annual: 'Mandatory direct discussion',
    alternate: 'For SaaS operation or deep product integration',
    formula: `You must contact ${CONTACT_EMAIL} to discuss terms.`,
  },
];

const pricingExamples = [
  {team: '6 seats', total: '$74/year'},
  {team: '12 seats', total: '$98/year'},
  {team: '20 seats', total: '$130/year'},
  {team: '21 seats', total: '$205/year'},
  {team: '50 seats', total: '$350/year'},
];

export default function PricingPage(): ReactNode {
  return (
    <Layout title="Pricing" description="Wavry commercial and hosted pricing overview.">
      <main className={styles.wrap}>
        <section className={styles.hero}>
          <p className={styles.eyebrow}>Pricing</p>
          <Heading as="h1" className={styles.title}>
            Commercial pricing is intentionally simple and low-cost.
          </Heading>
          <p className={styles.subtitle}>
            If you release your source under AGPL-3.0 terms, you can use Wavry for free.
            Commercial pricing applies when you want exclusion from AGPL obligations and want to use the Wavry cloud service.
          </p>
          <p className={styles.contactLine}>
            Contact: <a href={`mailto:${CONTACT_EMAIL}`}>{CONTACT_EMAIL}</a>
          </p>
          <div className={styles.actions}>
            <Link className="button button--primary" to="/docs/deployment-modes">
              Compare deployment modes
            </Link>
            <Link className="button button--outline" to="/docs/overview">
              Read platform overview
            </Link>
          </div>
        </section>

        <section className={styles.section}>
          <Heading as="h2" className={styles.sectionTitle}>
            Free option
          </Heading>
          <article className={styles.noticeCard}>
            <Heading as="h3" className={styles.noticeTitle}>
              Open source path (AGPL-3.0)
            </Heading>
            <p className={styles.noticeBody}>
              Teams can use Wavry at no cost when they follow AGPL requirements.
              Commercial licensing is optional and only needed if you want exclusion from those source-release obligations.
            </p>
          </article>
        </section>

        <section className={styles.section}>
          <Heading as="h2" className={styles.sectionTitle}>
            Commercial licensing + Wavry cloud service
          </Heading>
          <p className={styles.sectionIntro}>
            Pricing below covers the commercial licensing side described above.
            If a company wants to run Wavry as a SaaS service or integrate it directly into a service offering,
            direct contact is required.
          </p>
          <div className={styles.grid}>
            {commercialPlans.map((plan) => (
              <article key={plan.name} className={styles.card}>
                <p className={styles.cardStatus}>{plan.size}</p>
                <Heading as="h3" className={styles.cardTitle}>
                  {plan.name}
                </Heading>
                <p className={styles.cardMainPrice}>{plan.annual}</p>
                <p className={styles.cardAlt}>{plan.alternate}</p>
                <p className={styles.cardFormula}>{plan.formula}</p>
              </article>
            ))}
          </div>

          <div className={styles.examplesWrap}>
            <Heading as="h3" className={styles.examplesTitle}>
              Yearly total examples
            </Heading>
            <table className={styles.examplesTable}>
              <thead>
                <tr>
                  <th>Team Size</th>
                  <th>Yearly Total</th>
                </tr>
              </thead>
              <tbody>
                {pricingExamples.map((row) => (
                  <tr key={row.team}>
                    <td>{row.team}</td>
                    <td>{row.total}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </section>

        <section className={styles.section}>
          <Heading as="h2" className={styles.sectionTitle}>
            Nonprofit and public-good waiver
          </Heading>
          <article className={styles.noticeCard}>
            <Heading as="h3" className={styles.noticeTitle}>
              Free yearly license available on request
            </Heading>
            <p className={styles.noticeBody}>
              If your organization is using Wavry for nonprofit or public-good work, message the project owner.
              A yearly commercial license can be granted for free.
            </p>
            <p className={styles.noticeBody}>
              Contact: <a href={`mailto:${CONTACT_EMAIL}`}>{CONTACT_EMAIL}</a>
            </p>
          </article>
        </section>
      </main>
    </Layout>
  );
}
