package ocla

import (
	"bufio"
	"encoding/json"
	"io"
	"strings"
)

type StreamEvent struct {
	Type string
	Data json.RawMessage
}

func ParseSSEStream(reader io.Reader) <-chan StreamEvent {
	ch := make(chan StreamEvent, 16)
	go func() {
		defer close(ch)
		scanner := bufio.NewScanner(reader)
		var eventType string
		var dataLines []string
		for scanner.Scan() {
			line := scanner.Text()
			switch {
			case strings.HasPrefix(line, "event:"):
				eventType = strings.TrimSpace(line[6:])
			case strings.HasPrefix(line, "data:"):
				dataLines = append(dataLines, strings.TrimSpace(line[5:]))
			case line == "":
				if len(dataLines) > 0 {
					ch <- StreamEvent{
						Type: eventType,
						Data: json.RawMessage(strings.Join(dataLines, "\n")),
					}
				}
				eventType = ""
				dataLines = nil
			}
		}
	}()
	return ch
}
