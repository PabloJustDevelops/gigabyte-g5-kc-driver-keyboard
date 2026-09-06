import * as React from "react";
import * as SliderPrimitive from "@radix-ui/react-slider";
import { cn } from "../../lib/cn";

interface SliderProps {
  value: number;
  min?: number;
  max?: number;
  step?: number;
  disabled?: boolean;
  className?: string;
  /** Extra classes for the track (e.g. a gradient). */
  trackClassName?: string;
  /** Extra classes for the filled range. */
  rangeClassName?: string;
  /** Extra classes for the thumb. */
  thumbClassName?: string;
  ariaLabel?: string;
  onValueChange?: (value: number) => void;
  onValueCommit?: (value: number) => void;
}

export const Slider = React.forwardRef<HTMLSpanElement, SliderProps>(
  (
    {
      value,
      min = 0,
      max = 100,
      step = 1,
      disabled,
      className,
      trackClassName,
      rangeClassName,
      thumbClassName,
      ariaLabel,
      onValueChange,
      onValueCommit,
    },
    ref,
  ) => (
    <SliderPrimitive.Root
      ref={ref}
      className={cn(
        "relative flex w-full touch-none select-none items-center",
        disabled && "opacity-45",
        className,
      )}
      min={min}
      max={max}
      step={step}
      value={[value]}
      disabled={disabled}
      onValueChange={(vs) => {
        const v = vs[0];
        if (v !== undefined) onValueChange?.(v);
      }}
      onValueCommit={(vs) => {
        const v = vs[0];
        if (v !== undefined) onValueCommit?.(v);
      }}
      aria-label={ariaLabel}
    >
      <SliderPrimitive.Track
        className={cn(
          "relative h-1 w-full grow overflow-hidden rounded-full bg-primary/15",
          trackClassName,
        )}
      >
        <SliderPrimitive.Range
          className={cn("absolute h-full bg-primary", rangeClassName)}
        />
      </SliderPrimitive.Track>
      <SliderPrimitive.Thumb
        className={cn(
          "block h-3.5 w-3.5 rounded-full border-2 border-primary bg-background shadow-sm transition-colors",
          "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40",
          thumbClassName,
        )}
      />
    </SliderPrimitive.Root>
  ),
);
Slider.displayName = "Slider";
