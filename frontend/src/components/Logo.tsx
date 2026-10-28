import React from "react";
import Image from "next/image";

interface LogoProps {
  isCollapsed: boolean;
}

const Logo = ({ isCollapsed }: LogoProps) =>
  isCollapsed ? (
    <div className="flex items-center justify-center mb-2">
      <Image
        src="/logo-collapsed.png"
        alt="Meetily"
        width={40}
        height={40}
        className="object-contain"
        priority
      />
    </div>
  ) : (
    <div className="w-full text-lg text-center border rounded-full bg-blue-50 border-white font-semibold text-gray-700 mb-2">
      <span>Meetily</span>
    </div>
  );

export default Logo;
