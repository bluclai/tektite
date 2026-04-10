<script lang="ts">
    import { cva, type VariantProps } from "class-variance-authority";
    import { cn } from "$lib/utils";
    import type { Snippet } from "svelte";
    import type { HTMLButtonAttributes } from "svelte/elements";

    const buttonVariants = cva(
        "inline-flex items-center justify-center rounded-[2px] font-sans font-medium transition-all duration-200 ease-out cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed select-none",
        {
            variants: {
                variant: {
                    // Primary: gradient CTA with primary glow on hover
                    primary: [
                        "bg-primary",
                        "text-on-primary border-none",
                        "hover:shadow-[0_0_20px_color-mix(in_srgb,var(--color-primary)_20%,transparent)]",
                    ],
                    // Secondary: transparent + ghost border
                    secondary: [
                        "bg-transparent text-on-surface-variant",
                        "border border-[color-mix(in_srgb,var(--color-outline-variant)_15%,transparent)]",
                        "hover:bg-surface-container-high",
                    ],
                    // Ghost: no background, no border
                    ghost: [
                        "bg-transparent text-on-surface-variant border-none",
                        "hover:bg-surface-container-high",
                    ],
                    // Destructive
                    destructive: [
                        "bg-destructive text-white border-none",
                        "hover:bg-destructive/90",
                    ],
                },
                size: {
                    sm: "gap-1.5 text-xs px-2.5 h-7",
                    md: "gap-2 text-sm px-3 h-9",
                    lg: "gap-2 text-sm px-4 h-10",
                    // Icon: square, no padding, no gap — used with a single Lucide icon (size={16})
                    icon: "h-8 w-8 p-0 shrink-0",
                },
            },
            defaultVariants: {
                variant: "secondary",
                size: "md",
            },
        },
    );

    type Props = HTMLButtonAttributes &
        VariantProps<typeof buttonVariants> & {
            class?: string;
            children?: Snippet;
        };

    let {
        variant,
        size,
        class: className,
        children,
        ...rest
    }: Props = $props();
</script>

<button class={cn(buttonVariants({ variant, size }), className)} {...rest}>
    {@render children?.()}
</button>
