"use client"

import * as React from "react"
import { ContextMenu as ContextMenuPrimitive } from "radix-ui"

import { cn } from "@/lib/utils"

const ContextMenu = ContextMenuPrimitive.Root
const ContextMenuTrigger = ContextMenuPrimitive.Trigger
const ContextMenuSub = ContextMenuPrimitive.Sub

const contentClass =
  "z-50 min-w-[8rem] overflow-hidden rounded-md border border-border bg-popover p-1 text-popover-foreground shadow-md"

const ContextMenuContent = React.forwardRef<
  React.ElementRef<typeof ContextMenuPrimitive.Content>,
  React.ComponentPropsWithoutRef<typeof ContextMenuPrimitive.Content>
>(({ className, ...props }, ref) => (
  <ContextMenuPrimitive.Portal>
    <ContextMenuPrimitive.Content ref={ref} className={cn(contentClass, className)} {...props} />
  </ContextMenuPrimitive.Portal>
))
ContextMenuContent.displayName = "ContextMenuContent"

const ContextMenuSubContent = React.forwardRef<
  React.ElementRef<typeof ContextMenuPrimitive.SubContent>,
  React.ComponentPropsWithoutRef<typeof ContextMenuPrimitive.SubContent>
>(({ className, ...props }, ref) => (
  <ContextMenuPrimitive.Portal>
    <ContextMenuPrimitive.SubContent ref={ref} className={cn(contentClass, className)} {...props} />
  </ContextMenuPrimitive.Portal>
))
ContextMenuSubContent.displayName = "ContextMenuSubContent"

const itemClass =
  "relative flex cursor-default select-none items-center gap-2 rounded-sm px-2 py-1.5 text-sm outline-none focus:bg-accent focus:text-foreground data-[disabled]:pointer-events-none data-[disabled]:opacity-50"

const ContextMenuItem = React.forwardRef<
  React.ElementRef<typeof ContextMenuPrimitive.Item>,
  React.ComponentPropsWithoutRef<typeof ContextMenuPrimitive.Item> & { destructive?: boolean }
>(({ className, destructive, ...props }, ref) => (
  <ContextMenuPrimitive.Item
    ref={ref}
    className={cn(itemClass, destructive && "text-destructive focus:text-destructive", className)}
    {...props}
  />
))
ContextMenuItem.displayName = "ContextMenuItem"

const ContextMenuSubTrigger = React.forwardRef<
  React.ElementRef<typeof ContextMenuPrimitive.SubTrigger>,
  React.ComponentPropsWithoutRef<typeof ContextMenuPrimitive.SubTrigger>
>(({ className, ...props }, ref) => (
  <ContextMenuPrimitive.SubTrigger
    ref={ref}
    className={cn(itemClass, "data-[state=open]:bg-accent", className)}
    {...props}
  />
))
ContextMenuSubTrigger.displayName = "ContextMenuSubTrigger"

const ContextMenuSeparator = React.forwardRef<
  React.ElementRef<typeof ContextMenuPrimitive.Separator>,
  React.ComponentPropsWithoutRef<typeof ContextMenuPrimitive.Separator>
>(({ className, ...props }, ref) => (
  <ContextMenuPrimitive.Separator ref={ref} className={cn("-mx-1 my-1 h-px bg-border", className)} {...props} />
))
ContextMenuSeparator.displayName = "ContextMenuSeparator"

export {
  ContextMenu,
  ContextMenuTrigger,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuSub,
  ContextMenuSubTrigger,
  ContextMenuSubContent,
}
