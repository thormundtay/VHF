use super::super::{DurationOrEndTime, StartTime};
use crate::{ParseError, ParseResult};
use jiff::{SignedDuration, Span, Zoned};

/// This is the rounding tolerance for f64 to usize in TraceTimer.
const TOL: f64 = 0.0001;

#[derive(Debug)]
pub(super) struct TraceTimer {
    /// Timing associated with the 0th data point in the trace.
    trace_start: Zoned,
    /// Temporal interval between two consecutive samples within the trace.
    sample_interval: SignedDuration,
    /// Number of elements within the length file.
    trace_len: i64,
    pub plot_start: usize,
    pub plot_end: usize,
}

impl TraceTimer {
    pub(super) fn new(
        trace_start: Zoned,
        sample_interval: Span,
        trace_len: usize,
    ) -> ParseResult<Self> {
        if trace_len > i64::MAX as usize {
            // Assumes 64bit wide usize check.
            log::error!("Trace length too large!");
            return Err(ParseError::ValueError);
        }

        Ok(Self {
            trace_start,
            sample_interval: sample_interval.try_into().map_err(ParseError::JiffError)?,
            trace_len: trace_len as _,
            plot_start: 0,
            plot_end: trace_len,
        })
    }

    // Determine the index of an absolute time in the context of the trace. Guaranteed to be within
    // the trace.
    fn abs_as_index(&self, time: &Zoned) -> ParseResult<usize> {
        let rel = time.duration_since(&self.trace_start);
        debug_assert!(!rel.is_negative());
        let dt = self.sample_interval;

        let n = rel.div_duration_f64(dt);
        assert!((n - n.round()).abs() < TOL);
        Ok(n.round() as _)
    }

    // Determine the index of an absolute time in the context of the trace. Guaranteed to be within
    // the trace.
    fn span_as_offset(&self, duration: &Span) -> ParseResult<usize> {
        debug_assert!(!duration.is_negative());
        let dt = self.sample_interval;

        let d: SignedDuration = (*duration).try_into().map_err(ParseError::JiffError)?;

        let n = d.div_duration_f64(dt);
        assert!((n - n.round()).abs() < TOL);
        Ok(n.round() as _)
    }

    /// Updates the internal state of the plot window. Returns true if the internal state has
    /// changed.
    // See Python implementation and test for requirements and specification.
    pub(super) fn update_plot_timing(
        &mut self,
        start: Option<StartTime>,
        duration_or_end: Option<DurationOrEndTime>,
    ) -> ParseResult<bool> {
        // Guard clauses
        if start.is_none() && duration_or_end.is_none() {
            log::warn!("Nones were passed to update_plot_timing");
            return Ok(false);
        }
        if let Some(StartTime::Rel(ref s)) = start
            && s.is_negative()
        {
            log::warn!("Negative relative start time were passed to update_plot_timing");
            return Err(ParseError::ValueError);
        }
        if let Some(DurationOrEndTime::Rel(ref d)) = duration_or_end
            && d.is_negative()
        {
            log::warn!("Negative duration was passed to update_plot_timing");
            return Err(ParseError::ValueError);
        }

        let original_plot_start = self.plot_start;
        let original_plot_end = self.plot_end;
        let trace_end_max = self.trace_len as usize - 1;

        if let Some(ref start_param) = start {
            let new_start_idx = match start_param {
                StartTime::Abs(t) => self.abs_as_index(t)?,
                StartTime::Rel(r) => self.span_as_offset(r)?,
            };

            let coerced_start = new_start_idx.clamp(0, trace_end_max);
            if coerced_start != new_start_idx {
                log::debug!("Specified start {:?} was beyond end of trace.", &start);
            }

            self.plot_start = coerced_start;
        }

        if let Some(ref doe) = duration_or_end {
            let new_end_idx = match doe {
                DurationOrEndTime::Rel(d) => {
                    let duration_in_samples = self.span_as_offset(d)?;
                    self.plot_start.saturating_add(duration_in_samples)
                }
                DurationOrEndTime::Abs(e) => self.abs_as_index(e)?,
            };

            let coerced_end = new_end_idx.clamp(0, trace_end_max);
            if coerced_end != new_end_idx {
                log::debug!(
                    "Specified duration_or_end {:?} was beyond end of trace.",
                    &duration_or_end
                );
            }

            self.plot_end = coerced_end;
        }

        // Handle edge cases where start > end due to user input
        if start.is_some() && duration_or_end.is_none() {
            // If only start changed and end is now less than start, move end to start
            if self.plot_end < self.plot_start {
                self.plot_end = self.plot_start;
            }
        } else if start.is_none() && duration_or_end.is_some() {
            // If only end changed and start is now greater than end, move start to end
            if self.plot_start > self.plot_end {
                self.plot_start = self.plot_end;
            }
        }

        assert!(self.plot_start <= self.plot_end);

        Ok(self.plot_start != original_plot_start || self.plot_end != original_plot_end)
    }
}
