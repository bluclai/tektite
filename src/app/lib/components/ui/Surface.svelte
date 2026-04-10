<script lang="ts">
    import { cva, type VariantProps } from 'class-variance-authority';
    import { cn } from '$lib/utils';
    import type { Snippet } from 'svelte';
    import type { HTMLAttributes } from 'svelte/elements';

    // Surface hierarchy — each tier is 2-3% lighter than the one below.
    // Always place a higher tier on a lower one to create optical depth.
    //
    //   base        #131314  — main app background
    //   low         #1a1a1b  — sidebar, navigation panels
    //   container   #1f1f20  — inset sections within panels
    //   high        #2a2a2b  — selected states, hover targets
    //   highest     #353536  — modals, popovers, command palette
    const surfaceVariants = cva('', {
        variants: {
            level: {
                base: 'bg-surface',
                low: 'bg-surface-container-low',
                container: 'bg-surface-container',
                high: 'bg-surface-container-high',
                highest: 'bg-surface-container-highest',
            },
            // Glassmorphism — for floating elements laid over other surfaces
            glass: {
                true: 'backdrop-blur-[20px] bg-[color-mix(in_srgb,var(--color-surface-variant)_95%,transparent)]',
                false: '',
            },
        },
        defaultVariants: {
            level: 'base',
            glass: false,
        },
    });

    type Props = HTMLAttributes<HTMLDivElement> &
        VariantProps<typeof surfaceVariants> & {
            class?: string;
            children?: Snippet;
        };

    let { level, glass, class: className, children, ...rest }: Props = $props();
</script>

<div class={cn(surfaceVariants({ level, glass }), className)} {...rest}>
    {@render children?.()}
</div>
