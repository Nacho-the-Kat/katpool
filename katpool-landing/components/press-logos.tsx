import Image from 'next/image'
import Client1 from '@/public/images/client1.png'
import Client2 from '@/public/images/client2.png'
import Client3 from '@/public/images/client3.png'
import Client4 from '@/public/images/client4.png'
import Client5 from '@/public/images/client5.png'

export default function PressLogos() {
  return (
    <section>
      <div className="max-w-6xl mx-auto px-4 sm:px-6">
        <div className="py-6 border-b border-gray-800">
          {/* Items */}
          <div className="max-w-sm md:max-w-5xl mx-auto grid gap-2 grid-cols-4 md:grid-cols-5">
            {/* Item */}
            <div className="flex items-center justify-center py-2 col-span-2 md:col-auto" data-aos="zoom-out">
              <Image
                src={Client1}
                alt="Client 1"
                width={124}
                height={24}
                className="max-w-full"
              />
            </div>
            {/* Item */}
            <div className="flex items-center justify-center py-2 col-span-2 md:col-auto" data-aos="zoom-out" data-aos-delay="100">
              <Image
                src={Client2}
                alt="Client 2"
                width={124}
                height={24}
                className="max-w-full"
              />
            </div>
            {/* Item */}
            <div className="flex items-center justify-center py-2 col-span-2 md:col-auto" data-aos="zoom-out" data-aos-delay="200">
              <Image
                src={Client3}
                alt="Client 3"
                width={124}
                height={24}
                className="max-w-full"
              />
            </div>
            {/* Item */}
            <div className="flex items-center justify-center py-2 col-span-2 md:col-auto" data-aos="zoom-out" data-aos-delay="300">
              <Image
                src={Client4}
                alt="Client 4"
                width={124}
                height={24}
                className="max-w-full"
              />
            </div>
            {/* Item */}
            <div className="flex items-center justify-center py-2 col-span-2 md:col-auto col-start-2" data-aos="zoom-out" data-aos-delay="400">
              <Image
                src={Client5}
                alt="Client 5"
                width={124}
                height={24}
                className="max-w-full"
              />
            </div>
          </div>
        </div>
      </div>
    </section>
  )
}