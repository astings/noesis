import React from "react";
import Image from "next/image";

interface LogoProps {
  isCollapsed: boolean;
}

const Logo = ({ isCollapsed }: LogoProps) =>
  isCollapsed ? (
    <Image
      src="/logo-collapsed.png"
      alt="Meetily"
      width={28}
      height={28}
      className="object-contain"
      priority
    />
  ) : (
    <span>Meetily</span>
  );

export default Logo;
