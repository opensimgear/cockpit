// Normalized values can repeat even when the native sample changes.
// Retain extrema before matching visible transitions.
export function profileAxisRanges() {
  const axes = new Map();

  return {
    declare({id, min, max}) {
      if (!Number.isSafeInteger(min) || !Number.isSafeInteger(max) || min >= max) {
        throw Error(`Invalid native range for ${id}.`);
      }
      const previous = axes.get(id);
      if (previous) {
        if (min !== previous.declaredNative.min || max !== previous.declaredNative.max) {
          throw Error(`Native range changed for ${id}.`);
        }
        return;
      }
      axes.set(id, {
        id,
        declaredNative: {min, max},
        declaredPresentation: {min: min < 0 ? -32768 : 0, max: 32767},
        samples: 0,
        unavailable: 0,
        minimum: null,
        maximum: null,
      });
    },

    record(id, value, sequence) {
      const axis = axes.get(id);
      if (!axis) throw Error(`Missing native range for ${id}.`);
      if (!value || value.normalized === null) {
        axis.unavailable++;
        return;
      }
      const {native, normalized} = value;
      const {min, max} = axis.declaredNative;
      const {min: low, max: high} = axis.declaredPresentation;
      const expected = low + Math.trunc((native - min) * (high - low) / (max - min));
      if (!Number.isSafeInteger(native) || native < min || native > max || normalized !== expected) {
        throw Error(`Inconsistent native and presentation values for ${id}.`);
      }
      axis.samples++;
      if (!axis.minimum || native < axis.minimum.native) axis.minimum = {native, normalized, sequence};
      if (!axis.maximum || native > axis.maximum.native) axis.maximum = {native, normalized, sequence};
    },

    evidence() {
      return [...axes.values()];
    },
  };
}
