'use client';

import React from 'react';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';

interface MainContentProps {
  children: React.ReactNode;
}

const MainContent: React.FC<MainContentProps> = ({ children }) => {
  const { isCollapsed } = useSidebar();

  return (
    <main
      className={`mt-10 h-[calc(100vh-2.5rem)] flex-1 min-w-0 overflow-hidden rounded-tl-xl border-l border-t border-border bg-background transition-all duration-300 ${
        isCollapsed ? 'ml-12' : 'ml-[260px]'
      }`}
    >
      <div className="h-full min-w-0 w-full max-w-full overflow-hidden">
        {children}
      </div>
    </main>
  );
};

export default MainContent;
