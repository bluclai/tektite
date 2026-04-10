<script lang="ts">
    import { cva, type VariantProps } from 'class-variance-authority';
    import { cn } from '$lib/utils';
    import type { Snippet } from 'svelte';
    import type { HTMLAttributes } from 'svelte/elements';

    const cardVariants = cva('rounded-lg transition-colors duration-200 ease-out', {
        variants: {
            // Surface tier the card sits on — pick the tier one level above its parent
            tier: {
                low: 'bg-surface-container-low',
                default: 'bg-surface-container',
                high: 'bg-surface-container-high',
                highest: 'bg-surface-container-highest',
            },
            // Whether the card responds to hover (e.g. clickable cards)
            interactive: {
                true: 'cursor-pointer hover:brightness-110',
                false: '',
            },
            padding: {
                none: '',
                sm: 'p-3',
                md: 'p-4',
                lg: 'p-6',
            },
        },
        defaultVariants: {
            tier: 'low',
            interactive: false,
            padding: 'md',
        },
    });

    type Props = HTMLAttributes<HTMLDivElement> &
        VariantProps<typeof cardVariants> & {
            class?: string;
            children?: Snippet;
        };

    let { tier, interactive, padding, class: className, children, ...rest }: Props = $props();
</script>

<div class={cn(cardVariants({ tier, interactive, padding }), className)} {...rest}>
    {@render children?.()}
</div>
