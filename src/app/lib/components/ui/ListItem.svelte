<script lang="ts">
    import { cva, type VariantProps } from 'class-variance-authority';
    import { cn } from '$lib/utils';
    import type { Snippet } from 'svelte';
    import type { HTMLAttributes } from 'svelte/elements';

    // No dividers — separation comes from 4px vertical spacing + hover tonal shift.
    const listItemVariants = cva(
        'flex items-center w-full py-1 px-2 rounded-sm transition-colors duration-200 ease-out',
        {
            variants: {
                active: {
                    true: 'bg-surface-container-high text-on-surface',
                    false: 'text-on-surface-variant hover:bg-surface-container-high hover:text-on-surface',
                },
                size: {
                    sm: 'py-0.5 px-2 text-xs gap-1.5',
                    md: 'py-1 px-2 text-sm gap-2',
                    lg: 'py-1.5 px-2.5 text-sm gap-2',
                },
            },
            defaultVariants: {
                active: false,
                size: 'md',
            },
        },
    );

    type Props = HTMLAttributes<HTMLLIElement> &
        VariantProps<typeof listItemVariants> & {
            class?: string;
            children?: Snippet;
        };

    let { active, size, class: className, children, ...rest }: Props = $props();
</script>

<li class={cn(listItemVariants({ active, size }), className)} {...rest}>
    {@render children?.()}
</li>
