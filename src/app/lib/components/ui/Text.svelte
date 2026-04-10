<script lang="ts">
    import { cva, type VariantProps } from 'class-variance-authority';
    import { cn } from '$lib/utils';
    import type { Snippet } from 'svelte';

    // Typography scale from docs/design.md
    // Inter for UI; Bitter (prose) for editorial/editor content.
    const textVariants = cva('', {
        variants: {
            variant: {
                // Display — empty states, branding only
                'display-lg': 'font-prose text-[3.5rem] font-normal leading-[1.1]',
                // Headlines — Inter, tight tracking
                'headline-md': 'font-sans text-[1.75rem] font-medium leading-[1.2]',
                'headline-sm': 'font-sans text-[1.5rem] font-medium leading-[1.3] tracking-[-0.02em]',
                // Body
                'body-lg': 'font-sans text-base leading-[1.5]',
                'body-md': 'font-sans text-sm leading-[1.5]',
                // Prose — editor and long-form reading
                'prose-lg': 'font-prose text-base leading-[1.6]',
                // Label — uppercase micro-copy
                'label-lg': 'font-sans text-xs font-medium tracking-[0.06em] uppercase',
                'label-sm': 'font-sans text-[0.6875rem] font-medium tracking-[0.06em] uppercase',
            },
            color: {
                default: 'text-on-surface',
                muted: 'text-on-surface-variant',
                primary: 'text-primary',
                tertiary: 'text-tertiary',
            },
        },
        defaultVariants: {
            variant: 'body-lg',
            color: 'default',
        },
    });

    // Map variant to the correct semantic HTML element
    const elementMap: Record<string, string> = {
        'display-lg': 'h1',
        'headline-md': 'h2',
        'headline-sm': 'h3',
        'body-lg': 'p',
        'body-md': 'p',
        'prose-lg': 'p',
        'label-lg': 'span',
        'label-sm': 'span',
    };

    type Variant = NonNullable<VariantProps<typeof textVariants>['variant']>;
    type Color = NonNullable<VariantProps<typeof textVariants>['color']>;

    type Props = {
        variant?: Variant;
        color?: Color;
        as?: string;
        class?: string;
        children?: Snippet;
        [key: string]: unknown;
    };

    let { variant = 'body-lg', color = 'default', as, class: className, children, ...rest }: Props = $props();

    const tag = $derived(as ?? elementMap[variant ?? 'body-lg'] ?? 'span');
</script>

<svelte:element this={tag} class={cn(textVariants({ variant, color }), className)} {...rest}>
    {@render children?.()}
</svelte:element>
