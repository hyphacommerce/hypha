"use client";

// When the discount is live: active or a draft, and its window, in the store's time zone.

import { Card } from "@astryxdesign/core/Card";
import { DateTimeInput, type ISODateTimeString } from "@astryxdesign/core/DateTimeInput";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Switch } from "@astryxdesign/core/Switch";
import { Text } from "@astryxdesign/core/Text";

import { useTimeZone } from "@/components/date-time";

import { type SectionProps, fieldStatus } from "./fields";

/** What the date and time input takes: a wall-clock time to the minute. */
const isMinute = (text: string): text is ISODateTimeString =>
  /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}$/.test(text);

export function ScheduleSection({ draft, errors, onChange, isReadOnly }: SectionProps) {
  const timeZone = useTimeZone();
  const schedule = draft.schedule;
  return (
    <Card>
      <VStack gap={4}>
        <VStack gap={1}>
          <Text type="display-3" as="h2">
            Schedule
          </Text>
          <Text type="supporting" color="secondary">
            Times are in the store&apos;s time zone, {timeZone}.
          </Text>
        </VStack>
        <Switch
          label="Active"
          description="A draft never applies, whatever its dates say: switch it off to pause the discount."
          value={draft.status === "active"}
          onChange={(on) => onChange({ ...draft, status: on ? "active" : "draft" }, ["status"])}
          isDisabled={isReadOnly}
        />
        <HStack gap={4} wrap="wrap">
          <DateTimeInput
            label="Starts"
            description="Empty: at once."
            value={isMinute(schedule.startsAt) ? schedule.startsAt : undefined}
            onChange={(value) =>
              onChange({ ...draft, schedule: { ...schedule, startsAt: value ?? "" } }, [
                "schedule.startsAt",
              ])
            }
            hasClear
            hourFormat="24h"
            weekStartsOn="mon"
            isDisabled={isReadOnly}
            status={fieldStatus(errors["schedule.startsAt"])}
          />
          <DateTimeInput
            label="Ends"
            description="Empty: never."
            value={isMinute(schedule.endsAt) ? schedule.endsAt : undefined}
            onChange={(value) =>
              onChange({ ...draft, schedule: { ...schedule, endsAt: value ?? "" } }, [
                "schedule.endsAt",
              ])
            }
            hasClear
            hourFormat="24h"
            weekStartsOn="mon"
            isDisabled={isReadOnly}
            status={fieldStatus(errors["schedule.endsAt"])}
          />
        </HStack>
      </VStack>
    </Card>
  );
}
