import Link from 'next/link'
import Image from 'next/image'
import Logo from '../../public/images/widelogo.png'

export default function Footer() {
  return (
    <footer>
      <div className="max-w-6xl mx-auto px-4 sm:px-6">
        {/* Blocks */}
        <div className="grid sm:grid-cols-12 gap-8 py-8 md:py-12">
          {/* 1st block */}
          <div className="sm:col-span-12 lg:col-span-4 lg:max-w-xs">
            <div className="mb-2">
              {/* Logo */}
              <Link className="inline-flex" href="/" aria-label="Cruip">
                <Image src={Logo} alt="Logo" width={200} height={50} />
              </Link>
            </div>
          </div>
          {/* 
          // 2nd block 
          <div className="sm:col-span-6 md:col-span-3 lg:col-span-2">
            <h6 className="text-xs text-gray-200 font-semibold uppercase mb-2"></h6>
            <ul className="text-sm space-y-2">
              <li>
                <a className="text-gray-400 hover:text-customSecondary transition duration-150 ease-in-out" href="https://app.katpool.xyz">Get Connected</a>
              </li>
            </ul>
          </div>
          // 3rd block 
          <div className="sm:col-span-6 md:col-span-3 lg:col-span-2">
            <h6 className="text-xs text-gray-200 font-semibold uppercase mb-2"></h6>
            <ul className="text-sm space-y-2">
              <li>
                <a className="text-gray-400 hover:text-customSecondary transition duration-150 ease-in-out" href="#0">Terms of Use</a>
              </li>
            </ul>
          </div>
          // 4th block 
          <div className="sm:col-span-6 md:col-span-3 lg:col-span-2">
            <h6 className="text-xs text-gray-200 font-semibold uppercase mb-2"></h6>
            <ul className="text-sm space-y-2">
              <li>
                <a className="text-gray-400 hover:text-customSecondary transition duration-150 ease-in-out" href="#0">Privacy Policy</a>
              </li>
            </ul>
          </div>
          // 5th block 
          <div className="sm:col-span-6 md:col-span-3 lg:col-span-2">
            <h6 className="text-xs text-gray-200 font-semibold uppercase mb-2"></h6>
            <ul className="text-sm space-y-2">
              <li>
                <a className="text-gray-400 hover:text-customSecondary transition duration-150 ease-in-out" href="#0">© Nacho the Kat 2024</a>
              </li>
            </ul>
          </div>
          */}
        </div>
      </div>
    </footer>
  )
}
