export const metadata = {
  title: 'Kat Pool - Open Source Kaspa Mining',
  description: 'Kat Pool is the first open-source mining pool built for Kaspa, empowering miners with transparency, scalability, and freedom.',
}

import Hero from '@/components/hero'
import PressLogos from '@/components/press-logos'
import Features from '@/components/features'
import Pricing from '@/components/pricing'
import Resources from '@/components/resources'

export default function Home() {
  return (
    <>
      <Hero />
      <PressLogos />
      <Features />
      <Pricing />
      <Resources />
    </>
  )
}
