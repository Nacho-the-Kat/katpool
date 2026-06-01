import Link from 'next/link'
import Image from 'next/image'
import Logo from '../../public/images/widelogo.png'

export default function Header({ nav = false }: {
  nav?: boolean
}) {
  return (
    <header className="absolute w-full z-30">
      <div className="max-w-6xl mx-auto px-4 sm:px-6">
        <div className="flex items-center justify-between h-16 md:h-20">
          {/* Site branding */}
          <div className="shrink-0 mr-4">
            {/* Logo */}
            <Link className="block" href="/" aria-label="Cruip">
              <Image src={Logo} alt="Logo" width={200} height={50} />
            </Link>
          </div>

          {/* Desktop navigation */}
          {nav &&
            <nav className="flex grow">
              {/* Desktop sign in links */}
              <ul className="flex grow justify-end flex-wrap items-center">
                <li className="ml-3">
                  <Link className="btn-sm text-white bg-gradient-to-t from-customPrimary to-customPrimary hover:to-customSecondary w-full shadow-lg group" href="#0">
                    Get Started <span className="tracking-normal text-customSecondary group-hover:translate-x-0.5 transition-transform duration-150 ease-in-out ml-1">-&gt;</span>
                  </Link>
                </li>
              </ul>
            </nav>
          }
        </div>
      </div>
    </header>
  )
}
